#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, ComparisonOptions, DocumentLimits, SchemaKey, SchemaProvider, SchemaTypeLookup, compare_xt_xb_documents, parse_xb,
    parse_xt,
};

const KEY: &str = "SCH_3701212_37102_13006";

#[test]
fn current_profile_has_bounded_membership_and_exact_identity() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let provider = registry.provider_for_key(&SchemaKey::parse(KEY)?).ok_or("missing profile")?;
    let profile = provider.profile();
    assert_eq!(profile.definitions().len(), 41);
    assert_eq!(profile.definitions().map(|d| d.fields.len()).sum::<usize>(), 339);
    assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
    assert_eq!(profile.definition(54).ok_or("missing TORUS")?.fields.len(), 12);
    for kind in [3, 101, 110, 177, 205] {
        assert_eq!(provider.lookup_type("13006", kind), SchemaTypeLookup::Unknown);
    }
    for key in ["SCH_3701213_37102_13006", "SCH_3700000_36001", "SCH_3701229_37102_13006"] {
        assert!(!provider.supports_schema_key(&SchemaKey::parse(key)?));
    }
    Ok(())
}

#[test]
fn text_embedded_schemas_are_reencoded_in_binary_form() -> support::Result<()> {
    let provider = BuiltinProfileRegistry::compiled()?.provider_for_key(&SchemaKey::parse(KEY)?).ok_or("missing profile")?;
    for marker in ["255 ", "1 CZ", "1 DA6 values0 1 1 dTZ"] {
        let mut text = support::xt_header(KEY, 0);
        text.extend_from_slice(format!("82 {marker}2 1 7 11 1 0 ").as_bytes());
        let original = parse_xt(&text, &provider, DocumentLimits::default())?;
        let encoded = support::encode_text_document_nodes(KEY, &original.nodes)?;
        let decoded = parse_xb(&encoded, &provider, DocumentLimits::default())?;
        assert!(compare_xt_xb_documents(&original, &decoded, ComparisonOptions::default())?.equivalent);
        assert_eq!(support::encode_document_nodes(KEY, &decoded.nodes)?, encoded);
    }
    Ok(())
}

#[test]
fn text_base_schemas_do_not_emit_embedded_declarations() -> support::Result<()> {
    let provider = BuiltinProfileRegistry::compiled()?.provider_for_key(&SchemaKey::parse(support::KEY)?).ok_or("missing profile")?;
    let mut text = support::xt_header(support::KEY, 0);
    text.extend_from_slice(b"82 2 1 7 11 1 0 ");
    let original = parse_xt(&text, &provider, DocumentLimits::default())?;
    let encoded = support::encode_text_document_nodes(support::KEY, &original.nodes)?;
    let decoded = parse_xb(&encoded, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&original, &decoded, ComparisonOptions::default())?.equivalent);
    Ok(())
}
