//! Native durable cloud outbox. Network adapters may acknowledge a save only after `stage`
//! returns. Acknowledging publication removes only its queue record, never the captured data.

use std::collections::BTreeMap;

use solidcraft_cloud::{Error, Manifest, Result, Snapshot, digest};
use solidcraft_doc::Document;

mod management;
pub use management::prepare_change;

#[cfg(not(target_arch = "wasm32"))]
mod auth;
#[cfg(not(target_arch = "wasm32"))]
pub use auth::NativeAccess;
#[cfg(not(target_arch = "wasm32"))]
mod catalog;
#[cfg(not(target_arch = "wasm32"))]
mod git;
#[cfg(not(target_arch = "wasm32"))]
mod previews;
#[cfg(not(target_arch = "wasm32"))]
mod process;
#[cfg(not(target_arch = "wasm32"))]
mod provider;
#[cfg(not(target_arch = "wasm32"))]
pub use provider::ProviderRepository;
#[cfg(not(target_arch = "wasm32"))]
mod provision;
#[cfg(not(target_arch = "wasm32"))]
mod receive;
#[cfg(not(target_arch = "wasm32"))]
mod ssh;
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
    let mut snapshot = Snapshot::prepare(serde_json::to_vec_pretty(&metadata).map_err(invalid)?, assets)?;
    let title = if document.name.len() <= 512 && !document.name.chars().any(char::is_control) { &document.name } else { "" };
    snapshot.manifest.extra.insert("title".into(), title.into());
    Ok(snapshot)
}

/// Compatibility validation happens before any asset is fetched or decompressed.
pub fn check_document(bytes: &[u8]) -> Result<()> {
    if bytes.len() > solidcraft_cloud::MAX_MANIFEST_BYTES {
        return Err(invalid("document metadata too large"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(invalid)?;
    let format = value.get("format").and_then(serde_json::Value::as_str).ok_or_else(|| invalid("missing document format"))?;
    // Drawings have a separate schema and session kind (#3890). Never deserialize one as
    // a design: serde's defaults could otherwise turn it into an empty, writable model.
    if format.starts_with("solidcraft-drawing/") {
        return Err(Error::UpgradeRequired(format!("drawing schema {format}")));
    }
    let version = solidcraft_doc::format::version_of(format).ok_or_else(|| invalid("missing or invalid document format"))?;
    if version > solidcraft_doc::format::FORMAT {
        return Err(Error::UpgradeRequired(format!("document schema solidcraft/{version}")));
    }
    if version == solidcraft_doc::format::FORMAT
        && let Some(unknown) = value
            .get("format_pending")
            .and_then(serde_json::Value::as_array)
            .and_then(|names| names.iter().filter_map(serde_json::Value::as_str).find(|name| !solidcraft_doc::format::supports_change(version, name)))
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

/// Read only the disposable preview, without touching authoritative source assets.
pub fn read_thumbnail(manifest: &Manifest, read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Option<Vec<u8>>> {
    Ok(solidcraft_cloud::Thumbnail::png(manifest, read)?.filter(|png| crate::design::thumbnail::valid_header(png)))
}

pub fn open_document(manifest: &Manifest, document: &[u8], read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<Document> {
    // The envelope takes priority over schema parsing, including on resumed uploads.
    manifest.write()?;
    if manifest.deleted() {
        return Err(invalid("this cloud design is in Trash; restore a revision before opening it"));
    }
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
    fn point_cloud_payloads_round_trip_through_cloud_and_native_storage() {
        use solidcraft_doc::point_cloud::{Colors, Points};
        let mut doc = Document::new("Scan reference");
        let id = doc.allocate_id().unwrap();
        doc.point_clouds.push(
            serde_json::from_value(serde_json::json!({
                "id": id,
                "name": "Scan",
                "origin": solidcraft_geom::Vec3::ZERO,
                "points": Points::new(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0].into()),
                "colors": Colors(vec![255, 0, 0, 0, 255, 0].into())
            }))
            .unwrap(),
        );
        let snapshot = prepare_document(&doc).unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&snapshot.document).unwrap();
        let cloud = &metadata["point_clouds"][0];
        assert_eq!(cloud["points"], "");
        assert_eq!(cloud["colors"], "");
        assert!(cloud["points_blob"].as_str().unwrap().ends_with(".points"));
        let opened = open_document(&snapshot.manifest, &snapshot.document, |path| Ok(snapshot.objects[path].clone())).unwrap();
        assert_eq!(opened.to_json(), doc.to_json());
        let native = crate::write_design_container(&opened).unwrap();
        assert_eq!(crate::read_design(&native).unwrap().to_json(), doc.to_json());
    }

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

    #[test]
    fn released_pending_capabilities_remain_compatible_with_cloud_storage() {
        // A development build wrote these names before they folded into format 36.
        // Native opening already recognizes them; cloud publication must agree.
        for version in [36, solidcraft_doc::format::FORMAT] {
            let mut document: serde_json::Value = serde_json::from_str(&Document::new("released capabilities").to_json()).unwrap();
            document["format"] = serde_json::json!(solidcraft_doc::format::format_string(version));
            document["format_pending"] = serde_json::json!(["circle-edge-distances", "tapped-hole-thread-depth"]);
            let bytes = serde_json::to_vec(&document).unwrap();
            let native = Document::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap();
            check_document(&bytes).unwrap();
            let snapshot = Snapshot::prepare(bytes, BTreeMap::new()).unwrap();
            let opened = open_document(&snapshot.manifest, &snapshot.document, |_| panic!("no assets needed")).unwrap();
            assert_eq!(opened.to_json(), native.to_json());
        }
    }

    #[test]
    fn unknown_pending_capabilities_require_upgrade_before_assets_are_read() {
        let mut document: serde_json::Value = serde_json::from_str(&Document::new("future capabilities").to_json()).unwrap();
        document["format_pending"] = serde_json::json!(["future-cloud-document-capability"]);
        let snapshot = Snapshot::prepare(serde_json::to_vec(&document).unwrap(), BTreeMap::from([("blob".into(), vec![42; 100])])).unwrap();
        assert!(matches!(
            open_document(&snapshot.manifest, &snapshot.document, |_| panic!("must not read unsupported payloads")),
            Err(Error::UpgradeRequired(_))
        ));
    }

    #[test]
    fn separate_drawings_cannot_be_opened_as_empty_designs() {
        let snapshot = Snapshot::prepare(br#"{"format":"solidcraft-drawing/1","sheets":[]}"#.to_vec(), BTreeMap::new()).unwrap();
        let error = open_document(&snapshot.manifest, &snapshot.document, |_| panic!("must not fetch drawing resources")).unwrap_err();
        assert!(matches!(error, Error::UpgradeRequired(_)));
        assert!(error.to_string().starts_with("Upgrade SolidCraft to open or save this design"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod local;
#[cfg(not(target_arch = "wasm32"))]
pub use local::LocalOutbox;
#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
pub use host::NativeHost;
