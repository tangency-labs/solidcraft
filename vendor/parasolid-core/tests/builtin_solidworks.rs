#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{BuiltinProfileRegistry, DocumentLimits, FieldValue, SchemaKey, SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt};

const KEY: &str = "SCH_3701229_37102_13006";

#[test]
fn solidworks_profile_has_reviewed_world_and_exact_membership() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let profile = provider.profile();
    assert_eq!(profile.definitions().len(), 41);
    assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
    assert_eq!(profile.definition(101).ok_or("missing WORLD")?.fields.len(), 11);
    assert_eq!(provider.lookup_type("13006", 3), SchemaTypeLookup::Unknown);
    assert_eq!(provider.lookup_type("13006", 4), SchemaTypeLookup::Unknown);
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3701230_37102_13006")?));
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3000310_30000_13006")?));
    Ok(())
}

fn world_pair() -> support::Result<(Vec<u8>, Vec<u8>)> {
    let mut binary = support::xb_header(KEY, 0)?;
    binary.extend_from_slice(&[0, 101, 0xff]);
    binary.extend_from_slice(&support::pointer(7)?);
    for index in [0, 0, 23, 0, 0, 0, 0] {
        binary.extend_from_slice(&support::pointer(index)?);
    }
    binary.push(1);
    binary.extend_from_slice(&support::pointer(41)?);
    binary.extend_from_slice(&123_456_i32.to_be_bytes());
    binary.extend_from_slice(&789_i32.to_be_bytes());
    support::terminate(&mut binary);
    let mut text = support::xt_header(KEY, 0);
    text.extend_from_slice(b"101 255 7 0 0 23 0 0 0 0 T41 123456 789 1 0");
    Ok((binary, text))
}

#[test]
fn world_fields_match_text_binary_and_independent_encoding() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let (binary, text) = world_pair()?;
    let parsed = parse_xb(&binary, &provider, DocumentLimits::default())?;
    let textual = parse_xt(&text, &provider, DocumentLimits::default())?;
    let node = &parsed.nodes[0];
    assert_eq!(node.node_type, 101);
    assert_eq!(node.index, 7);
    assert_eq!(node.fields[2].values, [FieldValue::PointerIndex(23)]);
    assert_eq!(node.fields[7].values, [FieldValue::Logical(true)]);
    assert_eq!(node.fields[8].values, [FieldValue::PointerIndex(41)]);
    assert_eq!(node.fields[9].values, [FieldValue::Integer(Some(123_456))]);
    assert_eq!(node.fields[10].values, [FieldValue::Integer(Some(789))]);
    for (left, right) in node.fields.iter().zip(&textual.nodes[0].fields) {
        assert_eq!(left.values, right.values);
        assert!(left.byte_range.start >= node.byte_range.start);
        assert!(left.byte_range.end <= node.byte_range.end);
    }
    assert_eq!(support::encode_document_nodes(KEY, &parsed.nodes)?, binary);
    for end in 0..binary.len() {
        assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
    }
    let mut invalid = binary;
    invalid[node.fields[7].byte_range.start] = 2;
    assert!(parse_xb(&invalid, &provider, DocumentLimits::default()).is_err());
    Ok(())
}

#[test]
fn deltas_stop_before_interpreting_undocumented_base_records() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let mut input = support::xb_header(KEY, 0)?;
    let start = input.len();
    input.extend_from_slice(&[0, 3, 0xff, 0, 177, 0, 178]);
    let error = parse_xb(&input, &provider, DocumentLimits::default()).err().ok_or("unsupported delta was accepted")?;
    assert_eq!(error.kind().code(), "schema.unknown_base_type");
    assert_eq!(error.offset(), start + 2);
    Ok(())
}

#[test]
fn solidcraft_35102_profile_is_exact_and_reads_extended_indices() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let key = SchemaKey::parse("SCH_3501251_35102_13006")?;
    let provider = registry.provider_for_key(&key).ok_or("missing exact saved-solid profile")?;
    let profile = provider.profile();
    assert_eq!(profile.definitions().len(), 43);
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert_eq!(provider.lookup_type("13006", 3), SchemaTypeLookup::Unknown);
    assert_eq!(provider.lookup_type("13006", 4), SchemaTypeLookup::Unknown);
    assert!(!provider.supports_schema_key(&SchemaKey::parse("SCH_3501252_35102_13006")?));
    assert!(!provider.supports_schema_key(&SchemaKey::parse(KEY)?));
    let mut bytes = support::xb_header(key.raw(), 0)?;
    bytes.extend([0, 86, 0xff]);
    bytes.extend(2_i32.to_be_bytes());
    bytes.extend(support::pointer(100_000)?);
    for value in [1.25_f64, -2.5, 3.75, 4.0, 5.0, 6.0] {
        bytes.extend(value.to_be_bytes());
    }
    support::terminate(&mut bytes);
    let d = parse_xb(&bytes, &provider, DocumentLimits::default())?;
    assert_eq!(d.nodes[0].index, 100_000);
    assert_eq!(
        d.nodes[0].fields[0].values,
        [FieldValue::Vector([Some(1.25), Some(-2.5), Some(3.75)]), FieldValue::Vector([Some(4.0), Some(5.0), Some(6.0)])]
    );
    for end in 0..bytes.len() {
        assert!(parse_xb(&bytes[..end], &provider, DocumentLimits::default()).is_err());
    }
    assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
    Ok(())
}
