//! Native durable cloud outbox. Network adapters may acknowledge a save only after `stage`
//! returns. Acknowledging publication removes only its queue record, never the captured data.

use std::collections::BTreeMap;

use solidcraft_cloud::{Error, Manifest, Result, Snapshot, digest};
use solidcraft_doc::Document;

#[cfg(not(target_arch = "wasm32"))]
mod git;
#[cfg(not(target_arch = "wasm32"))]
mod process;
#[cfg(not(target_arch = "wasm32"))]
pub use git::{GitRepository, Published};

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::Invalid(error.to_string())
}

/// Uses the native package's authoritative serializer, without creating a ZIP or geometry cache.
pub fn prepare_document(document: &Document) -> Result<Snapshot> {
    let mut assets = BTreeMap::new();
    let bytes = crate::design::package::encode(document, crate::DesignCompression::Fast, |bytes, extension, _| {
        let path = format!("blobs/{}.{extension}", digest(bytes));
        assets.entry(path.clone()).or_insert_with(|| bytes.to_vec());
        Ok(serde_json::Value::String(path))
    })
    .map_err(invalid)?;
    let metadata: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    Snapshot::prepare(serde_json::to_vec_pretty(&metadata).map_err(invalid)?, assets)
}

/// Compatibility validation happens before any asset is fetched or decompressed.
pub fn check_document(bytes: &[u8]) -> Result<()> {
    if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
        return Err(invalid("document metadata too large"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(invalid)?;
    let version = value
        .get("format")
        .and_then(serde_json::Value::as_str)
        .and_then(solidcraft_doc::format::version_of)
        .ok_or_else(|| invalid("missing or invalid document format"))?;
    if version > solidcraft_doc::format::FORMAT {
        return Err(Error::UpgradeRequired(format!("document schema solidcraft/{version}")));
    }
    let pending = solidcraft_doc::format::pending_names();
    if version == solidcraft_doc::format::FORMAT
        && !pending.is_empty()
        && let Some(unknown) = value
            .get("format_pending")
            .and_then(serde_json::Value::as_array)
            .and_then(|names| names.iter().filter_map(serde_json::Value::as_str).find(|name| !pending.iter().any(|known| known == name)))
    {
        return Err(Error::UpgradeRequired(format!("document capability {unknown}")));
    }
    crate::design::check_hdr_manifest(bytes).map_err(invalid)?;
    // Embedded HDRs use blob placeholders. Full document validation follows hydration;
    // standalone JSON validation is valid only when those resources are absent.
    if value
        .get("render")
        .and_then(|render| render.get("resources"))
        .and_then(serde_json::Value::as_object)
        .is_none_or(|resources| resources.is_empty())
    {
        Document::from_json(std::str::from_utf8(bytes).map_err(invalid)?).map_err(invalid)?;
    }
    Ok(())
}

pub fn open_document(manifest: &Manifest, document: &[u8], read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Document> {
    // The envelope takes priority over schema parsing, including on resumed uploads.
    manifest.write()?;
    check_document(document)?;
    struct Assets(BTreeMap<String, Vec<u8>>);
    impl crate::design::package::BlobReader for Assets {
        fn blob(&mut self, value: &serde_json::Value, field: &str, _extension: &str) -> crate::Result<&[u8]> {
            let path = value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| crate::IoError::Invalid("missing cloud asset reference".into()))?;
            self.0.get(path).map(Vec::as_slice).ok_or_else(|| crate::IoError::Invalid(format!("missing cloud asset {path}")))
        }
    }
    let mut assets = Assets(Snapshot::hydrate(manifest, document, read)?);
    crate::design::package::decode(document, &mut assets).map_err(invalid)
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use solidcraft_doc::FeatureKind;

    #[test]
    fn cloud_round_trip_shares_native_payload_serialization_and_reuses_assets() {
        let mut doc = Document::new("Shared payload");
        doc.add_feature(
            FeatureKind::Import {
                file: "example.step".into(),
                step: "ISO-10303-21;\nEND-ISO-10303-21;".into(),
                components: Vec::new(),
                skip: Vec::new(),
            },
            None,
        )
        .unwrap();
        let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 4\n".to_vec();
        for _ in 0..8 {
            hdr.extend_from_slice(&[128, 64, 32, 129]);
        }
        let resource = solidcraft_doc::render_resources::HdrResource::new(hdr).unwrap();
        let id = resource.id().to_string();
        std::sync::Arc::make_mut(&mut doc.render).resources.insert(resource).unwrap();
        let first = prepare_document(&doc).unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&first.document).unwrap();
        assert_eq!(metadata["render"]["resources"][&id]["data"], "");
        assert!(metadata["render"]["resources"][&id]["hdr_blob"].as_str().unwrap().ends_with(".hdr"));
        let reopened = open_document(&first.manifest, &first.document, |path| Ok(first.objects[path].clone())).unwrap();
        assert_eq!(reopened.to_json(), doc.to_json());
        assert_eq!(crate::read_design(&crate::write_design_container(&reopened).unwrap()).unwrap().to_json(), doc.to_json());
        doc.name = "Changed parameters".into();
        let changed = prepare_document(&doc).unwrap();
        assert_eq!(first.objects, changed.objects);
        assert_ne!(first.document, changed.document);
    }

    #[test]
    fn newer_document_schema_requires_upgrade_before_assets_are_read() {
        let snapshot = Snapshot::prepare(br#"{"format":"solidcraft/4294967295"}"#.to_vec(), BTreeMap::new()).unwrap();
        assert!(matches!(open_document(&snapshot.manifest, &snapshot.document, |_| panic!("must not fetch assets")), Err(Error::UpgradeRequired(_))));
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::fs;
    use std::path::{Path, PathBuf};

    use solidcraft_cloud::{Error, PendingSave, Result, Snapshot, StagedSave};

    pub struct LocalOutbox {
        root: PathBuf,
    }

    fn storage(e: impl std::fmt::Display) -> Error {
        Error::Storage(e.to_string())
    }

    impl LocalOutbox {
        pub fn new(root: PathBuf) -> Result<Self> {
            crate::vfs::check(&root.to_string_lossy()).map_err(storage)?;
            fs::create_dir_all(&root).map_err(storage)?;
            Ok(Self { root: fs::canonicalize(root).map_err(storage)? })
        }

        fn path(&self, name: &str) -> Result<PathBuf> {
            solidcraft_cloud::check_path(name)?;
            let mut path = self.root.clone();
            for part in name.split('/') {
                path.push(part);
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err(storage("outbox paths must not be symbolic links"));
                }
            }
            crate::vfs::check(&path.to_string_lossy()).map_err(storage)?;
            Ok(path)
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            let max = if name.starts_with("objects/") { solidcraft_cloud::CHUNK_BYTES } else { solidcraft_cloud::MAX_MANIFEST_BYTES };
            crate::vfs::read(&self.path(name)?.to_string_lossy(), max).map_err(storage)
        }

        fn store(&self, name: &str, bytes: &[u8]) -> Result<()> {
            let path = self.path(name)?;
            if path.exists() {
                if self.read(name)? != bytes {
                    return Err(storage("existing outbox object differs; local copy retained"));
                }
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(storage)?;
            }
            crate::safe_write::write_atomic(&path, bytes, false).map_err(storage)
        }

        pub fn stage(&self, save: &StagedSave) -> Result<()> {
            save.validate()?;
            // Verify completeness before the first write, then persist the commit record last.
            for (path, bytes) in &save.objects {
                self.store(path, bytes)?;
            }
            self.store(&save.record_path, &save.record_bytes)
        }

        pub fn pending(&self) -> Result<Vec<PendingSave>> {
            let path = self.path("pending")?;
            if !path.exists() {
                return Ok(Vec::new());
            }
            let mut records = Vec::new();
            for entry in fs::read_dir(path).map_err(storage)? {
                let entry = entry.map_err(storage)?;
                if entry.path().extension().is_none_or(|e| e != "json") {
                    continue;
                }
                if records.len() >= 10_000 {
                    return Err(storage("too many queued saves"));
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let record = PendingSave::read(&self.read(&format!("pending/{name}"))?)?;
                if name != format!("{}.json", record.id) {
                    return Err(storage("pending save identifier mismatch"));
                }
                records.push(record);
            }
            PendingSave::ordered(records)
        }

        pub fn recover(&self, save: &PendingSave) -> Result<Snapshot> {
            save.recover(|p| self.read(p))
        }

        /// Call only after the repository adapter confirms this captured save was published.
        pub fn published(&self, save: &PendingSave) -> Result<()> {
            PendingSave::read(&serde_json::to_vec(save).map_err(storage)?)?;
            let path = self.path(&format!("pending/{}.json", save.id))?;
            let current = PendingSave::read(&crate::vfs::read(&path.to_string_lossy(), solidcraft_cloud::MAX_MANIFEST_BYTES).map_err(storage)?)?;
            if current.manifest.document_hash != save.manifest.document_hash {
                return Err(Error::Conflict);
            }
            fs::remove_file(path).map_err(storage)
        }

        pub fn root(&self) -> &Path {
            &self.root
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::BTreeMap;
        #[test]
        fn local_save_survives_reopening_until_publication_is_confirmed() {
            let root = std::env::temp_dir().join(format!(
                "solidcraft-outbox-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            let save = PendingSave::stage(
                &Default::default(),
                "project",
                "parts/design",
                None,
                Snapshot::prepare(b"{}".to_vec(), BTreeMap::from([("blob".into(), vec![42; 100])])).unwrap(),
            )
            .unwrap();
            LocalOutbox::new(root.clone()).unwrap().stage(&save).unwrap();
            let reopened = LocalOutbox::new(root.clone()).unwrap();
            let pending = reopened.pending().unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(reopened.recover(&pending[0]).unwrap().document, b"{}");
            // A failed or skipped upload leaves the queued record. Retrying stage is idempotent.
            reopened.stage(&save).unwrap();
            assert_eq!(reopened.pending().unwrap().len(), 1);
            reopened.published(&pending[0]).unwrap();
            assert!(reopened.pending().unwrap().is_empty());
            assert!(reopened.recover(&pending[0]).is_ok());
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::LocalOutbox;
