//! Independent wire examples; real local fixtures are checked by the Python runner.

#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileCoverage, BuiltinProfileRegistry, ComparisonOptions, DocumentLimits, ErrorKind, FieldValue, InMemorySchemaProvider, RawNode,
    SchemaKey, SchemaProviderResolution, compare_xb_documents, compare_xt_xb_documents, parse_xb, parse_xt, scan_xt_node_types,
    schema::profiles::onshape_sch30000,
};
use serde_json::{Value, json};
use support::{KEY, Result, pair, pointer, terminate, xb_header, xt_header};

fn registry() -> Result<BuiltinProfileRegistry> {
    Ok(BuiltinProfileRegistry::new(vec![onshape_sch30000()?])?)
}

fn parsed_pair(node_type: u16, length: Option<i32>, text: &[u8], binary: &[u8]) -> Result<[Vec<RawNode>; 2]> {
    let (xt, xb) = pair(node_type, length, text, binary)?;
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let left = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let right = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&left, &right, ComparisonOptions::default())?.equivalent);
    assert_eq!(left.nodes.len(), 1);
    assert_eq!(right.nodes.len(), 1);
    assert_eq!(left.terminator.byte_range.end, xt.len());
    assert_eq!(right.terminator.byte_range.end, xb.len());
    let encoded = support::encode_nodes(&right.nodes)?;
    let decoded = parse_xb(&encoded, &provider, DocumentLimits::default())?;
    assert!(
        compare_xb_documents(
            &right,
            &decoded,
            ComparisonOptions { absolute_tolerance: 0.0, relative_tolerance: 0.0, ..ComparisonOptions::default() }
        )?
        .equivalent
    );
    Ok([left.nodes, right.nodes])
}

fn values(node: &RawNode, ordinal: usize) -> Value {
    json!(node.fields[ordinal].values.iter().map(support::value_json).collect::<Vec<_>>())
}

fn integers_text(values: &[i32]) -> Vec<u8> {
    let mut text = Vec::new();
    for value in values {
        text.extend_from_slice(value.to_string().as_bytes());
        text.push(b' ');
    }
    text
}

/// Payloads are specified directly, independently of the profile's field table.
fn variable_example(node_type: u16, count: usize) -> Result<(Vec<u8>, Vec<u8>, Value)> {
    let (text, binary, expected) = match node_type {
        74 => {
            let values = &[0, 32_766, 32_767][..count];
            let mut binary = [i32::try_from(count)?.to_be_bytes(), 0_i32.to_be_bytes()].concat();
            binary.extend(pointer(0)?);
            for value in values {
                binary.extend(pointer(u32::try_from(*value)?)?);
            }
            ([format!("{count} 0 0 ").into_bytes(), integers_text(values)].concat(), binary, json!(values))
        }
        79 | 84 => {
            let bytes = b"Q A"[..count].to_vec();
            (bytes.clone(), bytes.clone(), json!(bytes))
        }
        80 => {
            let values = &[1, 2, 10][..count];
            let mut text = b"0 0 9000 1 2 3 4 5 6 7 8 0 FTFTFTFTFTFTFT".to_vec();
            text.extend(integers_text(values));
            let mut binary = pointer(0)?.repeat(2);
            binary.extend(9000_i32.to_be_bytes());
            binary.extend(1..=8);
            binary.extend(pointer(0)?);
            binary.extend([0, 1].repeat(7));
            for value in values {
                binary.push(u8::try_from(*value)?);
            }
            (text, binary, json!(values))
        }
        81 => {
            let values = &[82, 83, 98][..count];
            let mut pointers = vec![80, 12, 0, 0, 32_766, 32_767];
            pointers.extend(values);
            let mut binary = 7_i32.to_be_bytes().to_vec();
            for value in &pointers {
                binary.extend(pointer(u32::try_from(*value)?)?);
            }
            ([b"7 ".to_vec(), integers_text(&pointers)].concat(), binary, json!(values))
        }
        82 => {
            let values = &[0, i32::MIN, i32::MAX][..count];
            (integers_text(values), values.iter().flat_map(|v| v.to_be_bytes()).collect(), json!(values))
        }
        83 => {
            let values = &[Some(1.25_f64), None, Some(-2.5)][..count];
            let text = values.iter().map(|v| v.map_or_else(|| "?".to_owned(), |v| format!("{v} "))).collect::<String>().into_bytes();
            let binary = values.iter().flat_map(|v| v.unwrap_or(-3.14158e13).to_be_bytes()).collect();
            (text, binary, json!(values))
        }
        98 => {
            let values = &[0x8004_u16, 0xD83D, 0xDE00][..count];
            let signed = values.iter().map(|v| i32::from(*v) - 65_536).collect::<Vec<_>>();
            (integers_text(&signed), values.iter().flat_map(|v| v.to_be_bytes()).collect(), json!(values))
        }
        _ => return Err("not a variable example".into()),
    };
    Ok((text, binary, expected))
}

#[test]
fn compiled_registry_preserves_the_verified_v30_exact_key() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    assert_eq!(registry.len(), 33);
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing compiled profile")?;
    assert_eq!(provider.profile().metadata().profile_sha256, support::profile_hash(provider.profile())?);
    let (xt, xb) = pair(82, Some(0), b"", b"")?;
    let left = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let right = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&left, &right, ComparisonOptions::default())?.equivalent);
    for raw in ["SCH_3000001_30000", "SCH_3000000_30001", "SCH_3000000_30000_13006"] {
        let near = SchemaKey::parse(raw)?;
        assert!(registry.provider_for_key(&near).is_none());
        let mut bytes = support::xt_header(raw, 0);
        bytes.extend_from_slice(b"1 0 ");
        assert_eq!(parse_xt(&bytes, &provider, DocumentLimits::default()).err().map(|e| e.kind()), Some(ErrorKind::UnsupportedBuiltinSchemaKey));
    }
    Ok(())
}

#[test]
fn explicit_profile_has_exact_scope_and_stable_hash() -> Result<()> {
    let profile = onshape_sch30000()?;
    assert_eq!(profile.metadata().profile_id, "onshape-sch30000-r3");
    assert_eq!(profile.metadata().revision, 3);
    assert_eq!(profile.metadata().coverage, BuiltinProfileCoverage::VerifiedSubset);
    assert_eq!(profile.metadata().profile_sha256, support::profile_hash(&profile)?);
    assert_eq!(profile.definitions().count(), 44);
    assert_eq!(profile.definitions().map(|d| d.fields.len()).sum::<usize>(), 356);
    assert_eq!(profile.accepted_schema_keys().map(SchemaKey::raw).collect::<Vec<_>>(), [KEY]);
    assert!(BuiltinProfileRegistry::default().provider_for_key(&SchemaKey::parse(KEY)?).is_none());
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let (_, xb) = pair(82, Some(0), b"", b"")?;
    let doc = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(matches!(
        doc.schema_provider,
        SchemaProviderResolution::Builtin { profile_revision: 3, coverage: BuiltinProfileCoverage::VerifiedSubset, .. }
    ));
    Ok(())
}

#[test]
fn ellipse_and_sphere_preserve_wire_order_and_boundaries() -> Result<()> {
    // Independent bytes: V30 ellipse sense precedes centre (the published
    // April 2008 ELLIPSE struct lists these in the opposite order).
    for (kind, text, reals, expected) in [
        (
            32,
            "7 0 0 0 0 0 --1.25 2.5 -3.75 0 1 0 0 0 -1 4.5 2.25 ",
            vec![-1.25_f64, 2.5, -3.75, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 4.5, 2.25],
            vec![json!([[-1.25, 2.5, -3.75]]), json!([[0.0, 1.0, 0.0]]), json!([[0.0, 0.0, -1.0]]), json!([4.5]), json!([2.25])],
        ),
        (
            53,
            "7 0 0 0 0 0 --1.25 2.5 -3.75 4.5 0 1 0 0 0 -1 ",
            vec![-1.25, 2.5, -3.75, 4.5, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0],
            vec![json!([[-1.25, 2.5, -3.75]]), json!([4.5]), json!([[0.0, 1.0, 0.0]]), json!([[0.0, 0.0, -1.0]])],
        ),
    ] {
        let mut binary = 7_i32.to_be_bytes().to_vec();
        binary.extend(pointer(0)?.repeat(5));
        binary.push(b'-');
        binary.extend(reals.iter().flat_map(|v| v.to_be_bytes()));
        let nodes = parsed_pair(kind, None, text.as_bytes(), &binary)?;
        for (encoding, nodes) in nodes.iter().enumerate() {
            let node = &nodes[0];
            assert_eq!(node.fields.len(), 7 + expected.len());
            assert_eq!(values(node, 6), json!([b'-']));
            for (ordinal, expected) in expected.iter().enumerate() {
                assert_eq!(values(node, ordinal + 7), *expected);
            }
            if encoding == 1 {
                let start = node.fields[0].byte_range.start;
                assert_eq!(node.fields[6].byte_range, start + 14..start + 15);
                assert_eq!(node.fields[7].byte_range, start + 15..start + 39);
                assert_eq!(node.byte_range.end, start + binary.len());
            }
        }
        // Every incomplete field must fail, even with a valid termination marker.
        for end in 0..binary.len() {
            let (xt, xb) = pair(kind, None, b"7 0 0 0 0 0 -", &binary[..end])?;
            reject_pair(&xt, &xb, DocumentLimits::default(), None)?;
        }
    }
    Ok(())
}

#[test]
fn all_variable_types_accept_zero_one_and_many_elements() -> Result<()> {
    for node_type in [74, 79, 80, 81, 82, 83, 84, 98] {
        for count in [0, 1, 3] {
            let (text, binary, expected) = variable_example(node_type, count)?;
            for nodes in parsed_pair(node_type, Some(i32::try_from(count)?), &text, &binary)? {
                let node = &nodes[0];
                assert_eq!(node.variable_length, Some(u32::try_from(count)?));
                assert_eq!(values(node, node.fields.len() - 1), expected, "type {node_type}, N={count}");
                if node_type == 80 {
                    assert_eq!(values(node, 3), json!([1, 2, 3, 4, 5, 6, 7, 8]));
                    assert_eq!(values(node, 5), json!([false, true, false, true, false, true, false, true, false, true, false, true, false, true]));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn text_escapes_and_space_compression_preserve_array_lengths() -> Result<()> {
    for node_type in [79, 84] {
        for (text, binary) in [(b"\\9".as_slice(), b"         ".as_slice()), (b"\\0\\n\\r\\\\", b"\0\r\n\\")] {
            for nodes in parsed_pair(node_type, Some(i32::try_from(binary.len())?), text, binary)? {
                assert_eq!(values(&nodes[0], 0), json!(binary));
            }
        }
    }
    Ok(())
}

#[test]
fn unicode_signed_and_unsigned_tokens_preserve_all_code_units() -> Result<()> {
    let units = [0_u16, 32_767, 32_768, 32_772, 65_535];
    let binary = units.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>();
    for signed in [false, true] {
        let tokens = units.map(|v| if signed && v >= 32_768 { i32::from(v) - 65_536 } else { i32::from(v) });
        for nodes in parsed_pair(98, Some(5), &integers_text(&tokens), &binary)? {
            assert_eq!(values(&nodes[0], 0), json!(units));
        }
    }
    for nodes in parsed_pair(82, Some(1), b"?", &(-32_764_i32).to_be_bytes())? {
        assert_eq!(values(&nodes[0], 0), json!([null]));
    }
    Ok(())
}

#[test]
fn vector_is_one_group_with_three_components_including_null() -> Result<()> {
    for null in [false, true] {
        let mut binary = 4_i32.to_be_bytes().to_vec();
        binary.extend(pointer(0)?.repeat(4));
        let components = if null { [-3.14158e13_f64; 3] } else { [1.25, -2.5, 3.75] };
        binary.extend(components.into_iter().flat_map(f64::to_be_bytes));
        let text = if null { b"4 0 0 0 0 ?".as_slice() } else { b"4 0 0 0 0 1.25 -2.5 3.75 " };
        for nodes in parsed_pair(29, None, text, &binary)? {
            assert_eq!(nodes[0].fields.len(), 6);
            assert_eq!(values(&nodes[0], 5), if null { json!([[null, null, null]]) } else { json!([[1.25, -2.5, 3.75]]) });
        }
    }
    Ok(())
}

#[test]
fn header_like_array_values_do_not_change_record_boundaries() -> Result<()> {
    let numbers: [i32; 6] = [12, 1, 187, 81, 1, 2];
    let binary = numbers.into_iter().flat_map(i32::to_be_bytes).collect::<Vec<_>>();
    for nodes in parsed_pair(82, Some(6), &integers_text(&numbers), &binary)? {
        assert_eq!(values(&nodes[0], 0), json!(numbers));
    }
    Ok(())
}

fn reject_pair(xt: &[u8], xb: &[u8], limits: DocumentLimits, kinds: Option<[ErrorKind; 2]>) -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let errors = [parse_xt(xt, &provider, limits).err(), parse_xb(xb, &provider, limits).err()];
    for (i, error) in errors.iter().enumerate() {
        assert!(error.is_some(), "encoding {i} unexpectedly accepted");
        if let Some(kinds) = kinds {
            assert_eq!(error.as_ref().map(parasolid_core::ParseError::kind), Some(kinds[i]));
        }
    }
    Ok(())
}

#[test]
fn near_and_embedded_keys_are_rejected_even_without_data_records() -> Result<()> {
    let registry = registry()?;
    let selected = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&selected).ok_or("missing profile")?;
    for key in ["SCH_3000001_30000", "SCH_3000000_30001", "SCH_3000000_30000_30000"] {
        assert!(registry.provider_for_key(&SchemaKey::parse(key)?).is_none());
        let mut xt = xt_header(key, 0);
        let mut xb = xb_header(key, 0)?;
        xt.extend(b"1 0");
        terminate(&mut xb);
        reject_pair(&xt, &xb, DocumentLimits::default(), Some([ErrorKind::UnsupportedBuiltinSchemaKey; 2]))?;
        assert_eq!(
            scan_xt_node_types(&xt, &provider, DocumentLimits::default(), &[82]).err().map(|e| e.kind()),
            Some(ErrorKind::UnsupportedBuiltinSchemaKey)
        );
    }
    Ok(())
}

#[test]
fn uncovered_types_are_not_inferred_from_payloads() -> Result<()> {
    for node_type in [11, 110, 205] {
        let (xt, xb) = pair(node_type, None, b"12 1 187 ", &[0; 12])?;
        reject_pair(&xt, &xb, DocumentLimits::default(), Some([ErrorKind::BuiltinProfileUncoveredType; 2]))?;
    }
    Ok(())
}

#[test]
fn builtin_rejects_user_fields_without_restricting_caller_providers() -> Result<()> {
    let mut xt = xt_header(KEY, 1);
    let mut xb = xb_header(KEY, 1)?;
    // Point: local id, four references, coordinates, then one user integer.
    xt.extend(b"29 1 4 0 0 0 0 1 2 3 777 1 0");
    xb.extend(29_u16.to_be_bytes());
    xb.extend(pointer(1)?);
    xb.extend(4_i32.to_be_bytes());
    xb.extend(pointer(0)?.repeat(4));
    xb.extend([1.0_f64, 2.0, 3.0].into_iter().flat_map(f64::to_be_bytes));
    xb.extend(777_i32.to_be_bytes());
    terminate(&mut xb);
    reject_pair(&xt, &xb, DocumentLimits::default(), Some([ErrorKind::UnsupportedUserFields; 2]))?;
    let mut caller = InMemorySchemaProvider::new();
    for definition in onshape_sch30000()?.definitions() {
        caller.insert("30000", definition.clone());
    }
    let left = parse_xt(&xt, &caller, DocumentLimits::default())?;
    let right = parse_xb(&xb, &caller, DocumentLimits::default())?;
    assert_eq!(left.nodes[0].user_fields, right.nodes[0].user_fields);
    assert_eq!(left.nodes[0].user_fields.len(), 1);
    let limits = DocumentLimits { max_fields_per_type: 5, ..DocumentLimits::default() };
    for error in [parse_xt(&xt, &caller, limits).err(), parse_xb(&xb, &caller, limits).err(), scan_xt_node_types(&xt, &caller, limits, &[29]).err()] {
        assert_eq!(error.map(|e| e.kind()), Some(ErrorKind::LimitExceeded));
    }
    Ok(())
}

#[test]
fn invalid_array_counts_are_rejected() -> Result<()> {
    let (xt, xb) = pair(82, Some(10_000_001), b"", b"")?;
    reject_pair(&xt, &xb, DocumentLimits::default(), Some([ErrorKind::LimitExceeded; 2]))?;
    let (xt, xb) = pair(82, Some(-1), b"", b"")?;
    reject_pair(&xt, &xb, DocumentLimits::default(), Some([ErrorKind::InvalidTextToken, ErrorKind::InvalidVariableLength]))?;
    let (text, binary, _) = variable_example(80, 3)?;
    let (xt, xb) = pair(80, Some(4), &text, &binary)?;
    reject_pair(&xt, &xb, DocumentLimits::default(), None)?;
    Ok(())
}

#[test]
fn duplicate_indices_truncation_and_trailing_data_are_rejected() -> Result<()> {
    let (xt, xb) = pair(82, Some(1), b"42 ", &42_i32.to_be_bytes())?;
    let mut duplicate_text = xt[..xt.len() - 3].to_vec();
    duplicate_text.extend(b"82 0 1 1 0");
    let mut duplicate_binary = xb[..xb.len() - 4].to_vec();
    duplicate_binary.extend(82_u16.to_be_bytes());
    duplicate_binary.extend(0_i32.to_be_bytes());
    duplicate_binary.extend(pointer(1)?);
    terminate(&mut duplicate_binary);
    reject_pair(&duplicate_text, &duplicate_binary, DocumentLimits::default(), Some([ErrorKind::DuplicateNodeIndex; 2]))?;
    for removed in 1..=7 {
        reject_pair(&xt[..xt.len() - removed], &xb[..xb.len() - removed], DocumentLimits::default(), None)?;
    }
    let mut trailing_text = xt;
    let mut trailing_binary = xb;
    trailing_text.extend(b" X");
    trailing_binary.push(0);
    reject_pair(&trailing_text, &trailing_binary, DocumentLimits::default(), Some([ErrorKind::TrailingText, ErrorKind::TrailingBytes]))?;
    Ok(())
}

#[test]
fn declared_resource_limits_are_enforced() -> Result<()> {
    let (xt, xb) = pair(82, Some(3), b"1 2 3 ", &[0; 12])?;
    for limits in [
        DocumentLimits { max_file_size: 1, ..DocumentLimits::default() },
        DocumentLimits { max_string_bytes: 1, ..DocumentLimits::default() },
        DocumentLimits { max_variable_elements: 2, ..DocumentLimits::default() },
    ] {
        reject_pair(&xt, &xb, limits, Some([ErrorKind::LimitExceeded; 2]))?;
    }
    let (text, binary, _) = variable_example(80, 0)?;
    let (xt, xb) = pair(80, Some(0), &text, &binary)?;
    reject_pair(&xt, &xb, DocumentLimits { max_fields_per_type: 6, ..DocumentLimits::default() }, Some([ErrorKind::LimitExceeded; 2]))?;
    let mut two_text = xt[..xt.len() - 3].to_vec();
    two_text.extend(b"82 0 2 1 0");
    let mut two_binary = xb[..xb.len() - 4].to_vec();
    two_binary.extend(82_u16.to_be_bytes());
    two_binary.extend(0_i32.to_be_bytes());
    two_binary.extend(pointer(2)?);
    terminate(&mut two_binary);
    for limits in [DocumentLimits { max_nodes: 1, ..DocumentLimits::default() }, DocumentLimits { max_schema_types: 1, ..DocumentLimits::default() }]
    {
        reject_pair(&two_text, &two_binary, limits, Some([ErrorKind::LimitExceeded; 2]))?;
    }
    Ok(())
}

#[test]
fn reencoding_uses_decoded_values_and_detects_changes() -> Result<()> {
    let registry = registry()?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let (_, xb) = pair(82, Some(1), b"0 ", &0_i32.to_be_bytes())?;
    let original = parse_xb(&xb, &provider, DocumentLimits::default())?;
    let mut nodes = original.nodes.clone();
    nodes[0].byte_range = 0..0;
    nodes[0].fields[0].byte_range = 0..0;
    let encoded = support::encode_nodes(&nodes)?;
    assert_eq!(encoded, xb);
    nodes[0].fields[0].values[0] = FieldValue::Integer(Some(1));
    let changed = support::encode_nodes(&nodes)?;
    assert_ne!(changed, encoded);
    let decoded = parse_xb(&changed, &provider, DocumentLimits::default())?;
    assert!(!compare_xb_documents(&original, &decoded, ComparisonOptions::default())?.equivalent);
    Ok(())
}
