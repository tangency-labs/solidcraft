#[path = "support/sch30000.rs"]
mod support;
use parasolid_core::{BuiltinProfileRegistry, SchemaKey, SchemaProvider, SchemaTypeLookup};

#[test]
fn exact_v34_profile_pins_reviewed_base_and_rejects_nearby_keys() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse("SCH_3401212_34101_13006")?;
    let provider = registry.provider_for_key(&key).ok_or("missing V34 profile")?;
    let profile = provider.profile();
    assert_eq!(profile.metadata().profile_id, "icad-sch34101-13006-r3");
    assert_eq!(profile.metadata().revision, 3);
    assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
    assert_eq!(profile.definitions().len(), 40);
    // Revision 3 admits only the further types observed under this key.
    for kind in [45, 56, 68, 127, 128, 134, 135, 136, 137] {
        assert!(profile.definition(kind).is_some());
    }
    for kind in [59, 60, 124, 125, 126] {
        assert_eq!(provider.lookup_type("13006", kind), SchemaTypeLookup::Unknown);
    }
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert_eq!(provider.lookup_type("13006", 101), SchemaTypeLookup::Unknown);
    for nearby in ["SCH_3401213_34101_13006", "SCH_3401212_34100_13006", "SCH_3401212_34101_13007"] {
        assert!(!provider.supports_schema_key(&SchemaKey::parse(nearby)?));
        assert!(registry.provider_for_key(&SchemaKey::parse(nearby)?).is_none());
    }
    Ok(())
}

#[test]
fn unchanged_point_has_independent_text_binary_values() -> support::Result<()> {
    use parasolid_core::{DocumentLimits, parse_xb, parse_xt};
    let key = "SCH_3401212_34101_13006";
    let registry = BuiltinProfileRegistry::compiled()?;
    let key_value = SchemaKey::parse(key)?;
    let provider = registry.provider_for_key(&key_value).ok_or("missing profile")?;
    let mut binary = support::xb_header(key, 0)?;
    binary.extend_from_slice(&[0, 29, 255]);
    binary.extend(support::pointer(7)?);
    binary.extend(123_i32.to_be_bytes());
    binary.extend(support::pointer(0)?.repeat(4));
    for value in [13.25_f64, -17.5, 23.75] {
        binary.extend(value.to_be_bytes());
    }
    support::terminate(&mut binary);
    let mut text = support::xt_header(key, 0);
    text.extend(b"29 255 7 123 0 0 0 0 13.25 -17.5 23.75 1 0");
    let a = parse_xb(&binary, &provider, DocumentLimits::default())?;
    let b = parse_xt(&text, &provider, DocumentLimits::default())?;
    for (left, right) in a.nodes[0].fields.iter().zip(&b.nodes[0].fields) {
        assert_eq!(left.values, right.values);
    }
    assert_eq!(a.nodes[0].fields[5].values, [parasolid_core::FieldValue::Vector([Some(13.25), Some(-17.5), Some(23.75)])]);
    assert_eq!(support::encode_document_nodes(key, &a.nodes)?, binary);
    for end in 0..binary.len() {
        assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
    }
    Ok(())
}

#[test]
fn unreviewed_base_type_is_not_guessed_from_an_embedded_marker() -> support::Result<()> {
    use parasolid_core::{DocumentLimits, parse_xb};
    let key = "SCH_3401212_34101_13006";
    let registry = BuiltinProfileRegistry::compiled()?;
    let k = SchemaKey::parse(key)?;
    let provider = registry.provider_for_key(&k).ok_or("profile")?;
    let mut bytes = support::xb_header(key, 0)?;
    bytes.extend_from_slice(&[0, 110, 255]);
    bytes.extend(support::pointer(1)?);
    support::terminate(&mut bytes);
    let error = parse_xb(&bytes, &provider, DocumentLimits::default()).err().ok_or("accepted unknown type")?;
    assert_eq!(error.kind().code(), "schema.unknown_base_type");
    Ok(())
}

#[test]
fn torus_base_preserves_authored_values_and_binary_boundaries() -> support::Result<()> {
    use parasolid_core::{DocumentLimits, FieldValue, parse_xb, parse_xt};
    let key = "SCH_3401212_34101_13006";
    let registry = BuiltinProfileRegistry::compiled()?;
    let provider = registry.provider_for_key(&SchemaKey::parse(key)?).ok_or("profile")?;
    let mut binary = support::xb_header(key, 0)?;
    binary.extend_from_slice(&[0, 54, 255]);
    binary.extend(support::pointer(7)?);
    binary.extend(123_i32.to_be_bytes());
    binary.extend(support::pointer(0)?.repeat(5));
    binary.push(b'+');
    for value in [1.0_f64, 2.0, 3.0, 0.0, 0.0, 1.0, 13.0, 2.5, 1.0, 0.0, 0.0] {
        binary.extend(value.to_be_bytes());
    }
    support::terminate(&mut binary);
    let mut text = support::xt_header(key, 0);
    text.extend(b"54 255 7 123 0 0 0 0 0 +1 2 3 0 0 1 13 2.5 1 0 0 1 0");
    let a = parse_xb(&binary, &provider, DocumentLimits::default())?;
    let b = parse_xt(&text, &provider, DocumentLimits::default())?;
    assert_eq!(a.nodes.len(), 1);
    for (left, right) in a.nodes[0].fields.iter().zip(&b.nodes[0].fields) {
        assert_eq!(left.values, right.values);
    }
    assert_eq!(a.nodes[0].fields[9].values, [FieldValue::Double(Some(13.0))]);
    assert_eq!(a.nodes[0].fields[10].values, [FieldValue::Double(Some(2.5))]);
    assert_eq!(support::encode_document_nodes(key, &a.nodes)?, binary);
    // The semantic role gate accepts this exact profile revision, then reports
    // that this deliberately isolated surface has no owning body.
    let error = parasolid_core::brep::map_xb_brep(&a).err().ok_or("accepted bodyless surface")?;
    assert_eq!(error.kind(), parasolid_core::ErrorKind::MissingBrepBody);
    for end in 0..binary.len() {
        assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
    }
    Ok(())
}
