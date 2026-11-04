use std::collections::BTreeMap;

use super::{
    encode_asset_header, encode_design_asset, encode_top_level, generated_design_asset,
    parse_asset_header, parse_top_level, push_ascii, push_u32, push_utf16, resolve_design_folder,
    AssetKind, DESIGN_ASSET_TYPE, GENERATED_DESIGN_ASSET_BASE, GENERATED_DOCUMENT_ASSET_GUID,
    GENERATED_DOCUMENT_GUID, TOP_LEVEL_MANIFEST_VERSION,
};
use cadmpeg_core::CodecError;

const DESIGN_GUID: &str = "10000000-0000-4000-8000-000000000001";
const OTHER_GUID: &str = "20000000-0000-4000-8000-000000000002";
const SECONDARY_GUID: &str = "30000000-0000-4000-8000-000000000003";

#[test]
fn current_top_level_manifest_round_trips() {
    let bytes = encode_top_level(DESIGN_GUID, &["Design Base", "Simulation"]).unwrap();
    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base", "Simulation"]);
}

#[test]
fn legacy_top_level_manifest_accepts_both_terminal_forms() {
    let mut bytes = Vec::new();
    push_ascii(&mut bytes, "3-2-0-0").unwrap();
    push_ascii(&mut bytes, "FusionDocType").unwrap();
    push_utf16(&mut bytes, ".f3d").unwrap();
    push_utf16(&mut bytes, "Fusion Document").unwrap();
    push_utf16(&mut bytes, "A Fusion Document").unwrap();
    push_utf16(&mut bytes, GENERATED_DOCUMENT_GUID).unwrap();
    push_utf16(&mut bytes, GENERATED_DOCUMENT_ASSET_GUID).unwrap();
    push_u32(&mut bytes, 0x0800_06e1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 1);
    push_utf16(&mut bytes, OTHER_GUID).unwrap();
    push_utf16(&mut bytes, DESIGN_GUID).unwrap();
    push_u32(&mut bytes, 2);
    push_utf16(&mut bytes, "Simulation").unwrap();
    push_utf16(&mut bytes, "Design Base").unwrap();
    push_u32(&mut bytes, 0);

    let word_only = parse_top_level(&bytes).unwrap();
    assert_eq!(word_only.asset_folder_bases, ["Simulation", "Design Base"]);

    bytes.push(0);
    push_utf16(&mut bytes, "Legacy Document").unwrap();

    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Simulation", "Design Base"]);

    push_utf16(&mut bytes, "urn:synthetic:lineage").unwrap();
    let with_lineage = parse_top_level(&bytes).unwrap();
    assert_eq!(
        with_lineage.asset_folder_bases,
        ["Simulation", "Design Base"]
    );
}

#[test]
fn top_level_manifest_reads_a_secondary_asset_folder_group() {
    fn manifest(secondary: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_ascii(&mut bytes, "3-2-0-0").unwrap();
        push_ascii(&mut bytes, "FusionDocType").unwrap();
        push_utf16(&mut bytes, ".f3d").unwrap();
        push_utf16(&mut bytes, "Fusion Document").unwrap();
        push_utf16(&mut bytes, "A Fusion Document").unwrap();
        push_utf16(&mut bytes, GENERATED_DOCUMENT_GUID).unwrap();
        push_utf16(&mut bytes, GENERATED_DOCUMENT_ASSET_GUID).unwrap();
        push_u32(&mut bytes, 0x0800_06e1);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_utf16(&mut bytes, DESIGN_GUID).unwrap();
        push_u32(&mut bytes, 2);
        push_utf16(&mut bytes, "Design Base").unwrap();
        push_utf16(&mut bytes, "Toolpath").unwrap();
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, secondary).unwrap();
        bytes.push(1);
        push_utf16(&mut bytes, "NA_EXPORT").unwrap();
        bytes
    }

    let parsed = parse_top_level(&manifest("Simulation Model")).unwrap();
    assert_eq!(
        parsed.asset_folder_bases,
        ["Design Base", "Toolpath", "Simulation Model"]
    );
    assert!(parse_top_level(&manifest("Toolpath")).is_err());
}

#[test]
fn top_level_manifest_accepts_a_terminal_export_flag_without_a_marker() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut marker = Vec::new();
    push_utf16(&mut marker, "NA_EXPORT").unwrap();
    assert!(bytes.ends_with(&marker));
    bytes.truncate(bytes.len() - marker.len());

    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
}

#[test]
fn top_level_manifest_export_flag_can_carry_a_lineage_urn() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut marker = Vec::new();
    push_utf16(&mut marker, "NA_EXPORT").unwrap();
    bytes.truncate(bytes.len() - marker.len());
    let flagged = bytes.clone();

    push_utf16(&mut bytes, "urn:synthetic:dm.lineage:key").unwrap();
    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);

    let mut not_urn = flagged;
    push_utf16(&mut not_urn, "NA_IMPORT").unwrap();
    assert!(parse_top_level(&not_urn).is_err());
}

#[test]
fn design_folder_uses_root_fusion_asset_not_active_guid_or_run_order() {
    let manifest =
        parse_top_level(&encode_top_level(OTHER_GUID, &["Simulation", "Design Base"]).unwrap())
            .unwrap();
    let mut entries = BTreeMap::new();
    entries.insert(
        "Simulation/Manifest.dat".to_string(),
        encode_fusion_subtype_asset("Simulation", OTHER_GUID, "Simulation").unwrap(),
    );
    entries.insert("Simulation/Breps.BlobParts/decoy.smbh".to_string(), vec![0]);
    entries.insert(
        "Design Base[Active]/Manifest.dat".to_string(),
        encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
    );
    entries.insert(
        "Design Base[Active]/Design1/BulkStream.dat".to_string(),
        vec![1],
    );

    let folder = resolve_design_folder(&manifest, entries.keys().map(String::as_str), |name| {
        entries.get(name).map(Vec::as_slice)
    })
    .unwrap();
    assert_eq!(folder, "Design Base[Active]");
}

#[test]
fn active_guid_can_be_shared_by_a_non_design_asset() {
    let manifest =
        parse_top_level(&encode_top_level(DESIGN_GUID, &["Simulation", "Design Base"]).unwrap())
            .unwrap();
    let entries = BTreeMap::from([
        (
            "Simulation/Manifest.dat".to_string(),
            encode_asset_header(
                "Simulation",
                DESIGN_GUID,
                SECONDARY_GUID,
                "SimulationAssetType",
            )
            .unwrap(),
        ),
        (
            "Design Base/Manifest.dat".to_string(),
            encode_design_asset("Design Base", DESIGN_GUID).unwrap(),
        ),
        ("Design Base/Design1/BulkStream.dat".to_string(), vec![1]),
    ]);
    let folder = resolve_design_folder(&manifest, entries.keys().map(String::as_str), |name| {
        entries.get(name).map(Vec::as_slice)
    })
    .unwrap();
    assert_eq!(folder, "Design Base");
}

/// Replace the leading version field, keeping every later byte.
fn with_version(bytes: &[u8], version: &str) -> Vec<u8> {
    let mut supported = Vec::new();
    push_ascii(&mut supported, TOP_LEVEL_MANIFEST_VERSION).unwrap();
    assert!(bytes.starts_with(&supported));
    let mut replacement = Vec::new();
    push_ascii(&mut replacement, version).unwrap();
    [replacement.as_slice(), &bytes[supported.len()..]].concat()
}

#[test]
fn an_unknown_version_is_parsed_with_the_known_layout() {
    // Version-only drift: the attempt runs the same parse the known version
    // runs, and the reading of the version survives to the classifier.
    let known = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let bytes = with_version(&known, "3-3-0-0");

    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
    assert_eq!(manifest.declared_version(), Some("3-3-0-0"));
}

#[test]
fn a_broken_layout_is_malformed_for_every_declared_version() {
    let known = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut anchor = Vec::new();
    push_ascii(&mut anchor, "FusionDocType").unwrap();
    let at = known
        .windows(anchor.len())
        .position(|window| window == anchor)
        .expect("the kind anchor is present");
    let mut moved = Vec::new();
    push_ascii(&mut moved, "FusionDocTypX").unwrap();
    for version in [TOP_LEVEL_MANIFEST_VERSION, "3-3-0-0"] {
        let mut bytes = with_version(&known, version);
        bytes.splice(at..at + anchor.len(), moved.clone());
        let error = parse_top_level(&bytes).unwrap_err();
        assert!(
            matches!(&error, CodecError::Malformed(message)
                    if message.contains(version) && message.contains("grammar does not fit")),
            "expected a structural failure naming version {version}, found {error:?}"
        );
    }
}

#[test]
fn top_level_manifest_version_requires_framing_but_recovers_unreadable_payload() {
    let complete = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();

    let truncated = parse_top_level(&complete[..6]).unwrap_err();
    assert!(
        matches!(truncated, CodecError::Malformed(_)),
        "expected a malformed truncation, found {truncated:?}"
    );

    let mut non_ascii = complete.clone();
    non_ascii[4] = 0x01;
    let recovered = parse_top_level(&non_ascii).unwrap();
    assert_eq!(recovered.declared_version(), None);
    assert_eq!(recovered.asset_folder_bases, ["Design Base"]);
    assert_eq!(recovered.diagnostics[0].field, "top-level manifest version");
}

#[test]
fn top_level_manifest_rejects_trailing_bytes() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    bytes.push(0);
    assert!(parse_top_level(&bytes).is_err());
}

#[test]
fn top_level_manifest_reports_unframed_suffix_after_lineage_urn() {
    let original = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut terminal = vec![1];
    push_utf16(&mut terminal, "NA_EXPORT").unwrap();

    for version in [TOP_LEVEL_MANIFEST_VERSION, "3-3-0-0"] {
        let mut bytes = with_version(&original, version);
        assert!(bytes.ends_with(&terminal));
        bytes.truncate(bytes.len() - terminal.len());
        bytes.push(0);
        push_utf16(&mut bytes, OTHER_GUID).unwrap();
        push_utf16(&mut bytes, "urn:synthetic:lineage").unwrap();
        assert_eq!(
            parse_top_level(&bytes).unwrap().asset_folder_bases,
            ["Design Base"]
        );

        let suffix_offset = bytes.len();
        bytes.extend_from_slice(&[0xa5; 6]);
        let error = parse_top_level(&bytes).unwrap_err();
        assert!(
            matches!(&error, CodecError::Malformed(message)
            if message.contains(version)
                && message.contains("grammar does not fit")
                && message.contains("top-level manifest terminal: 6 trailing byte(s)")
                && message.contains(&format!(
                    "no exact tail framing is valid; furthest candidate stopped at stream offset {suffix_offset}:"
                ))),
            "expected the terminal-consumption error and offset, found {error:?}"
        );
    }
}

#[test]
fn top_level_manifest_accepts_the_offline_save_marker() {
    let original = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut export = vec![1];
    push_utf16(&mut export, "NA_EXPORT").unwrap();
    assert!(original.ends_with(&export));
    let base = original[..original.len() - export.len()].to_vec();

    let mut offline = base.clone();
    offline.push(1);
    push_utf16(&mut offline, "NA_OFFLINESAVE").unwrap();
    assert!(parse_top_level(&offline).is_ok());

    // The marker is exact: a near spelling is a malformed lineage URN.
    let mut near = base;
    near.push(1);
    push_utf16(&mut near, "NA_OFFLINE").unwrap();
    assert!(parse_top_level(&near).is_err());
}

#[test]
fn top_level_manifest_reports_invalid_lineage_urn() {
    let original = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut terminal = vec![1];
    push_utf16(&mut terminal, "NA_EXPORT").unwrap();

    for lineage in ["synthetic:lineage", OTHER_GUID] {
        let mut bytes = original.clone();
        assert!(bytes.ends_with(&terminal));
        bytes.truncate(bytes.len() - terminal.len());
        bytes.push(0);
        push_utf16(&mut bytes, "Synthetic Document").unwrap();
        push_utf16(&mut bytes, lineage).unwrap();

        let error = parse_top_level(&bytes).unwrap_err();
        assert!(
            matches!(&error, CodecError::Malformed(message)
            if message.contains("top-level manifest lineage URN")
                && message.contains("value is not a nonempty ASCII URN")
                && message.contains(&format!(
                    "no exact tail framing is valid; furthest candidate stopped at stream offset {}:",
                    bytes.len()
                ))),
            "expected the lineage validation error and offset, found {error:?}"
        );
    }
}

#[test]
fn generated_asset_manifest_has_a_joinable_header() {
    let bytes = generated_design_asset().unwrap();
    let header = parse_asset_header(&bytes).unwrap();
    assert!(header.base_name.eq_str(GENERATED_DESIGN_ASSET_BASE));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_zero_design_asset_has_no_named_capability_registry() {
    let mut bytes = encode_asset_header(
        "Legacy Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 3);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 6);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Design").unwrap();
    push_ascii(&mut bytes, "Design").unwrap();

    let header = parse_asset_header(&bytes).unwrap();
    assert!(header.base_name.eq_str("Legacy Design"));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_ten_design_asset_carries_linked_document_triples() {
    let mut bytes = encode_asset_header(
        "Linked Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 10);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Application").unwrap();
    push_u32(&mut bytes, 52);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_u32(&mut bytes, 2);
    push_utf16(&mut bytes, "synthetic_urn:synthetic:version:1").unwrap();
    push_utf16(&mut bytes, DESIGN_GUID).unwrap();
    push_utf16(&mut bytes, OTHER_GUID).unwrap();
    push_u32(&mut bytes, 2);
    push_u32(&mut bytes, 5);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Design").unwrap();
    push_ascii(&mut bytes, "Design").unwrap();

    let header = parse_asset_header(&bytes).unwrap();
    assert!(header.base_name.eq_str("Linked Design"));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn revision_ten_design_asset_accepts_the_unlinked_root_tail() {
    for revision in [3, 11] {
        let mut bytes = encode_asset_header(
            "Root Design",
            DESIGN_GUID,
            SECONDARY_GUID,
            DESIGN_ASSET_TYPE,
        )
        .unwrap();
        push_u32(&mut bytes, 10);
        push_u32(&mut bytes, 0);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        for word in [0, revision, 1, 0] {
            push_u32(&mut bytes, word);
        }
        push_ascii(&mut bytes, "Design").unwrap();
        push_ascii(&mut bytes, "Design").unwrap();

        let header = parse_asset_header(&bytes).unwrap();
        assert!(header.base_name.eq_str("Root Design"));
        assert_eq!(
            header.kind,
            AssetKind::Design {
                fusion_subtype: None
            }
        );

        // A root entry still requires both role tokens and exact consumption.
        assert!(parse_asset_header(&bytes[..bytes.len() - 1]).is_err());
        bytes.push(0);
        assert!(parse_asset_header(&bytes).is_err());
    }
}

#[test]
fn revision_fourteen_uses_the_ascii_subtype_header() {
    let mut bytes =
        encode_asset_header("Design 14", DESIGN_GUID, SECONDARY_GUID, DESIGN_ASSET_TYPE).unwrap();
    push_u32(&mut bytes, 14);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_ascii(&mut bytes, "").unwrap();

    let header = parse_asset_header(&bytes).unwrap();
    assert!(header.base_name.eq_str("Design 14"));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn current_revisions_use_the_current_asset_header() {
    for revision in [11, 12, 13, 14, 15, 19, 20] {
        let mut bytes = encode_asset_header(
            "Intermediate Design",
            DESIGN_GUID,
            SECONDARY_GUID,
            DESIGN_ASSET_TYPE,
        )
        .unwrap();
        push_u32(&mut bytes, revision);
        push_u32(&mut bytes, 1);
        push_ascii(&mut bytes, "Application").unwrap();
        push_u32(&mut bytes, 139);
        push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
        bytes.push(0);
        push_ascii(&mut bytes, "").unwrap();

        let header = parse_asset_header(&bytes).unwrap();
        assert!(header.base_name.eq_str("Intermediate Design"));
        assert_eq!(
            header.kind,
            AssetKind::Design {
                fusion_subtype: None
            }
        );
    }
}

#[test]
fn an_unknown_revision_uses_the_current_asset_header() {
    let mut bytes = encode_asset_header(
        "Future Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 99);
    push_u32(&mut bytes, 1);
    push_ascii(&mut bytes, "Application").unwrap();
    push_u32(&mut bytes, 139);
    push_ascii(&mut bytes, "Neutron3DAssetType").unwrap();
    bytes.push(0);
    push_ascii(&mut bytes, "").unwrap();

    let header = parse_asset_header(&bytes).unwrap();
    assert!(header.base_name.eq_str("Future Design"));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn a_broken_unknown_revision_names_the_revision_as_the_probable_cause() {
    let mut bytes = encode_asset_header(
        "Future Design",
        DESIGN_GUID,
        SECONDARY_GUID,
        DESIGN_ASSET_TYPE,
    )
    .unwrap();
    push_u32(&mut bytes, 99);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "MovedAssetType").unwrap();

    let error = parse_asset_header(&bytes).unwrap_err();
    assert!(
        matches!(&error, CodecError::Malformed(message)
                if message.contains("declared revision 99 is the probable cause")),
        "expected a structural failure naming the revision, found {error:?}"
    );
}

fn encode_fusion_subtype_asset(
    base_name: &str,
    primary_guid: &str,
    subtype: &str,
) -> Result<Vec<u8>, CodecError> {
    let mut bytes =
        encode_asset_header(base_name, primary_guid, SECONDARY_GUID, DESIGN_ASSET_TYPE)?;
    push_u32(&mut bytes, 20);
    push_u32(&mut bytes, 0);
    push_ascii(&mut bytes, "Neutron3DAssetType")?;
    bytes.push(0);
    push_ascii(&mut bytes, subtype)?;
    Ok(bytes)
}

#[test]
fn manifest_discarded_fields_and_failed_tails_keep_declared_folders() {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut anchor = Vec::new();
    push_utf16(&mut anchor, DESIGN_GUID).unwrap();
    let at = bytes
        .windows(anchor.len())
        .rposition(|window| window == anchor)
        .unwrap();
    let mut failed = Vec::new();
    for _ in 0..32 {
        push_utf16(&mut failed, OTHER_GUID).unwrap();
        push_u32(&mut failed, 1);
        push_utf16(&mut failed, "Discarded Folder").unwrap();
        push_u32(&mut failed, 99);
    }
    bytes.splice(at..at, failed);
    let manifest = parse_top_level(&bytes).unwrap();
    assert_eq!(
        manifest.declared_version(),
        Some(TOP_LEVEL_MANIFEST_VERSION)
    );
    assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
    let asset = generated_design_asset().unwrap();
    let header = parse_asset_header(&asset).unwrap();
    assert!(header.base_name.eq_str(GENERATED_DESIGN_ASSET_BASE));
    assert_eq!(
        header.kind,
        AssetKind::Design {
            fusion_subtype: None
        }
    );
}

#[test]
fn descriptive_manifest_payload_recovery_preserves_folder_selection() {
    let original = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    // Counted ASCII version/kind, then counted UTF-16 extension and four metadata fields.
    let mut cursor = 0;
    for _ in 0..2 {
        let count = usize::try_from(u32::from_le_bytes(
            original[cursor..cursor + 4].try_into().unwrap(),
        ))
        .expect("fixture string length fits usize");
        cursor += 4 + count;
    }
    for index in 0..5 {
        let count = usize::try_from(u32::from_le_bytes(
            original[cursor..cursor + 4].try_into().unwrap(),
        ))
        .expect("fixture string length fits usize");
        if index > 0 {
            for malformed in [0xd800u16, u16::from(b'Z')] {
                if index < 3 && malformed == u16::from(b'Z') {
                    continue;
                }
                let mut source = original.clone();
                source[cursor + 4..cursor + 6].copy_from_slice(&malformed.to_le_bytes());
                let recovered = parse_top_level(&source).unwrap();
                assert_eq!(recovered.asset_folder_bases, ["Design Base"]);
                assert_eq!(recovered.diagnostics.len(), 1);
                let mut truncated = source;
                truncated[cursor..cursor + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                assert!(parse_top_level(&truncated).is_err());
            }
        }
        cursor += 4 + 2 * count;
    }
}

fn terminal_manifest(flag: u8, display_name: Option<&str>, lineage: &str) -> Vec<u8> {
    let mut bytes = encode_top_level(DESIGN_GUID, &["Design Base"]).unwrap();
    let mut export = vec![1];
    push_utf16(&mut export, "NA_EXPORT").unwrap();
    assert!(bytes.ends_with(&export));
    bytes.truncate(bytes.len() - export.len());
    bytes.push(flag);
    if let Some(name) = display_name {
        push_utf16(&mut bytes, name).unwrap();
    }
    push_utf16(&mut bytes, lineage).unwrap();
    bytes
}

#[test]
fn top_level_manifest_accepts_exact_lineage_suffix() {
    for version in [TOP_LEVEL_MANIFEST_VERSION, "3-3-0-0"] {
        let original = terminal_manifest(0, Some("Assembly"), "urn:synthetic:lineage");
        let mut bytes = with_version(&original, version);
        bytes.extend_from_slice(&[72, 0, 65, 0, 65, 0, 72, 0, 65, 0]);
        let manifest = parse_top_level(&bytes).unwrap();
        assert_eq!(manifest.asset_folder_bases, ["Design Base"]);
        assert_eq!(manifest.declared_version(), Some(version));
        assert!(manifest.diagnostics.is_empty());
    }
}

#[test]
fn top_level_manifest_lineage_suffix_requires_exact_bytes_and_eof() {
    let original = terminal_manifest(0, Some("Assembly"), "urn:synthetic:lineage");
    let suffix = [72, 0, 65, 0, 65, 0, 72, 0, 65, 0];
    for len in 1..suffix.len() {
        let mut bytes = original.clone();
        bytes.extend_from_slice(&suffix[..len]);
        assert!(parse_top_level(&bytes).is_err(), "suffix length {len}");
    }
    for at in 0..suffix.len() {
        let mut changed = suffix;
        changed[at] ^= 1;
        let mut bytes = original.clone();
        bytes.extend_from_slice(&changed);
        assert!(parse_top_level(&bytes).is_err(), "suffix byte {at}");
    }
    let mut bytes = original;
    bytes.extend_from_slice(&suffix);
    bytes.push(0);
    assert!(parse_top_level(&bytes).is_err());
}

#[test]
fn top_level_manifest_lineage_suffix_requires_display_name_and_lineage() {
    for mut bytes in [
        terminal_manifest(1, None, "urn:synthetic:lineage"),
        terminal_manifest(1, None, "NA_EXPORT"),
        terminal_manifest(1, None, "NA_OFFLINESAVE"),
        terminal_manifest(0, Some(""), "urn:synthetic:lineage"),
        terminal_manifest(0, Some("Assembly"), "synthetic:lineage"),
        terminal_manifest(0, None, "Assembly"),
    ] {
        bytes.extend_from_slice(&[72, 0, 65, 0, 65, 0, 72, 0, 65, 0]);
        assert!(parse_top_level(&bytes).is_err());
    }
}
