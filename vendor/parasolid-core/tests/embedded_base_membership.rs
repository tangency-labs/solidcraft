//! Independent embedded bytes exercise partial-base membership in both readers.

#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinProfileRegistry, BuiltinSchemaProfile, ComparisonOptions, DocumentLimits,
    EffectiveSchemaRegistry, ErrorKind, FieldDefinition, FieldType, FieldValue, InMemorySchemaProvider, SchemaKey, SchemaLimits, SchemaProvider,
    SchemaSource, SchemaTypeLookup, TypeDefinition, compare_xt_xb_documents, parse_xb, parse_xt, scan_xt_node_types,
};
use support::{Result, pointer, terminate, xb_header, xt_header};

const KEY: &str = "SCH_3000310_30000_13006";

fn metadata() -> BuiltinProfileMetadata {
    BuiltinProfileMetadata {
        profile_id: "synthetic-embedded-base".to_owned(),
        revision: 1,
        provider_schema: "13006".to_owned(),
        producer_scope: "synthetic tests only".to_owned(),
        coverage: BuiltinProfileCoverage::VerifiedSubset,
        evidence_manifest_sha256: None,
        profile_sha256: "0".repeat(64),
    }
}

fn definitions() -> Vec<TypeDefinition> {
    let field = |name: &str, field_type, count, transmitted| FieldDefinition {
        name: name.to_owned(),
        field_type,
        pointer_class: 0,
        element_count: count,
        transmitted,
    };
    vec![
        TypeDefinition::from_fields(12, "FIXED", "", vec![field("value", FieldType::Integer, 0, true)], SchemaSource::Base),
        TypeDefinition::from_fields(
            70,
            "VARIABLE",
            "",
            vec![
                field("keep", FieldType::Integer, 0, true),
                field("drop", FieldType::Integer, 0, true),
                // Effective base fields include this retained, non-transmitted tail.
                field("retained", FieldType::OpaquePointer, 1, false),
            ],
            SchemaSource::Base,
        ),
    ]
}

fn registry() -> Result<BuiltinProfileRegistry> {
    Ok(BuiltinProfileRegistry::new(vec![BuiltinSchemaProfile::new_embedded(metadata(), vec![KEY.to_owned()], definitions(), vec![203], vec![204])?])?)
}

fn pair(kind: u16, text: &[u8], binary: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut xt = xt_header(KEY, 0);
    xt.extend(format!("{kind} ").as_bytes());
    xt.extend(text);
    let mut xb = xb_header(KEY, 0)?;
    xb.extend(kind.to_be_bytes());
    xb.extend(binary);
    xt.extend(b"1 0 ");
    terminate(&mut xb);
    Ok((xt, xb))
}

// One fixed integer field, node index 1, value 91. Type 204 is known absent.
fn full_blob() -> Result<Vec<u8>> {
    let mut data = b"\x01\x03NEW\x00\x05value\x00\x00\x00\x01\x01d".to_vec();
    data.extend(pointer(1)?);
    data.extend(91_i32.to_be_bytes());
    Ok(data)
}

#[test]
fn membership_is_explicit_and_scoped_to_the_exact_base() -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing provider")?;
    assert!(matches!(provider.lookup_type("13006", 12), SchemaTypeLookup::Defined(_)));
    assert_eq!(provider.lookup_type("13006", 203), SchemaTypeLookup::PresentUnsupported);
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert_eq!(provider.lookup_type("13006", 205), SchemaTypeLookup::Unknown);
    assert_eq!(provider.lookup_type("30000", 204), SchemaTypeLookup::Unknown);
    assert_eq!(provider.profile().unsupported_base_types().collect::<Vec<_>>(), [203]);
    assert_eq!(provider.profile().absent_base_types().collect::<Vec<_>>(), [204]);
    for other in ["SCH_3000311_30000_13006", "SCH_3000310_30100_13006", "SCH_3000310_30000_13007"] {
        assert!(registry.provider_for_key(&SchemaKey::parse(other)?).is_none());
    }
    Ok(())
}

#[test]
fn unknown_and_unsupported_types_never_enter_the_full_decoder() -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing provider")?;
    for (kind, expected) in [(203, ErrorKind::UnsupportedBaseSchemaType), (205, ErrorKind::UnknownBaseSchemaType)] {
        // These valid full declarations formerly succeeded when None meant absent.
        let (xt, xb) = pair(kind, b"1 3 NEW0 5 value0 0 1 d1 91 ", &full_blob()?)?;
        let text = parse_xt(&xt, &provider, DocumentLimits::default()).err().ok_or("text accepted")?;
        let binary = parse_xb(&xb, &provider, DocumentLimits::default()).err().ok_or("binary accepted")?;
        assert_eq!(text.kind(), expected);
        assert_eq!(binary.kind(), expected);
        assert_eq!(binary.offset(), xb_header(KEY, 0)?.len() + 2);
        assert_eq!(text.offset(), xt_header(KEY, 0).len() + kind.to_string().len() + 1);
        assert_eq!(scan_xt_node_types(&xt, &provider, DocumentLimits::default(), &[]).err().map(|e| e.kind()), Some(expected));
        // Membership is checked before even reading an unchanged/delta/full marker.
        for bytes in [vec![], vec![255], b"\x01CZ".to_vec()] {
            let mut schemas = EffectiveSchemaRegistry::default();
            let error = schemas.resolve_first(&bytes, 0, kind, &key, &provider, SchemaLimits::default()).err().ok_or("schema accepted")?;
            assert_eq!(error.kind(), expected);
            assert!(schemas.is_empty());
        }
    }
    Ok(())
}

#[test]
fn confirmed_absence_allows_full_definition_and_caches_it() -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing provider")?;
    let mut binary = full_blob()?;
    binary.extend(204_u16.to_be_bytes());
    binary.extend(pointer(2)?);
    binary.extend((-17_i32).to_be_bytes());
    let (xt, xb) = pair(204, b"1 3 NEW0 5 value0 0 1 d1 91 204 2 -17 ", &binary)?;
    let a = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let b = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&a, &b, ComparisonOptions::default())?.equivalent);
    assert_eq!(b.nodes.len(), 2);
    assert_eq!(b.nodes[0].definition.source, SchemaSource::EmbeddedFull);
    assert!(b.nodes[1].first_schema.is_none());
    assert_eq!(b.nodes[1].fields[0].values, [FieldValue::Integer(Some(-17))]);
    Ok(())
}

#[test]
fn defined_types_reuse_unchanged_and_apply_all_delta_operations() -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing provider")?;
    let mut unchanged = vec![255];
    unchanged.extend(pointer(1)?);
    unchanged.extend((-19_i32).to_be_bytes());
    let (xt, xb) = pair(12, b"255 1 -19 ", &unchanged)?;
    let a = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let b = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&a, &b, ComparisonOptions::default())?.equivalent);
    assert_eq!(b.nodes[0].definition.source, SchemaSource::EmbeddedUnchanged);

    // C keep, D obsolete, I real, D non-transmitted variable, A byte array, Z.
    let mut delta = b"\x03CDI\x04real\x00\x00\x00\x01\x01fDA\x04tail\x00\x00\x00\x02\x01u\x01Z".to_vec();
    delta.extend(2_i32.to_be_bytes());
    delta.extend(pointer(1)?);
    delta.extend(7_i32.to_be_bytes());
    delta.extend(2.5_f64.to_be_bytes());
    delta.extend([4, 9]);
    let (xt, xb) = pair(70, b"3 CDI4 real0 0 1 fDA4 tail0 1 1 uTZ2 1 7 2.5 4 9 ", &delta)?;
    let a = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let b = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&a, &b, ComparisonOptions::default())?.equivalent);
    assert_eq!(b.nodes[0].definition.source, SchemaSource::EmbeddedDelta);
    assert_eq!(b.nodes[0].fields[0].values, [FieldValue::Integer(Some(7))]);
    assert_eq!(b.nodes[0].fields[1].values, [FieldValue::Double(Some(2.5))]);
    assert_eq!(b.nodes[0].fields[2].values, [FieldValue::UnsignedByte(4), FieldValue::UnsignedByte(9)]);
    assert_eq!(b.nodes[0].variable_length, Some(2));
    for end in 0..delta.len() {
        let (_, cut) = pair(70, b"", &delta[..end])?;
        assert!(parse_xb(&cut, &provider, DocumentLimits::default()).is_err(), "prefix {end}");
    }
    Ok(())
}

#[test]
fn complete_caller_catalogs_keep_their_existing_absence_contract() -> Result<()> {
    let mut provider = InMemorySchemaProvider::new();
    provider.add_schema("13006");
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    let (xt, xb) = pair(204, b"1 3 NEW0 5 value0 0 1 d1 91 ", &full_blob()?)?;
    let a = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let b = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&a, &b, ComparisonOptions::default())?.equivalent);
    Ok(())
}

#[test]
fn embedded_profile_construction_rejects_ambiguous_membership_and_keys() {
    for (present, absent) in
        [(vec![12], vec![]), (vec![], vec![12]), (vec![203, 203], vec![]), (vec![203], vec![203]), (vec![], vec![204, 204]), (vec![1], vec![])]
    {
        assert!(BuiltinSchemaProfile::new_embedded(metadata(), vec![KEY.to_owned()], definitions(), present, absent).is_err());
    }
    for key in ["SCH_3000310_13006", "SCH_3000310_30000_13007"] {
        assert!(BuiltinSchemaProfile::new_embedded(metadata(), vec![key.to_owned()], definitions(), vec![], vec![]).is_err());
    }
    assert!(BuiltinSchemaProfile::new(metadata(), vec![KEY.to_owned()], definitions()).is_err());
}
