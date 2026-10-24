//! M8.4 synthetic value ranges; these are not producer-generated holdouts.

#[path = "support/sch30000.rs"]
mod support;

use std::fmt::Write as _;

use parasolid_core::{
    BuiltinProfileRegistry, ComparisonOptions, DocumentLimits, FieldValue, SchemaKey, compare_xb_documents, compare_xt_xb_documents, parse_xb,
    parse_xt, schema::profiles::onshape_sch30000,
};
use support::{KEY, Result, pair};

fn check_pair(node_type: u16, length: i32, text: &[u8], binary: &[u8], ordinal: usize, expected: &[FieldValue]) -> Result<()> {
    let (xt, xb) = pair(node_type, Some(length), text, binary)?;
    let registry = BuiltinProfileRegistry::new(vec![onshape_sch30000()?])?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    let left = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let right = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&left, &right, ComparisonOptions::default())?.equivalent);
    assert_eq!(left.nodes[0].fields[ordinal].values, expected);
    assert_eq!(right.nodes[0].fields[ordinal].values, expected);
    assert_eq!(left.terminator.byte_range.end, xt.len());
    assert_eq!(right.terminator.byte_range.end, xb.len());
    let bytes = support::encode_nodes(&right.nodes)?;
    let encoded = parse_xb(&bytes, &provider, DocumentLimits::default())?;
    assert!(
        compare_xb_documents(
            &right,
            &encoded,
            ComparisonOptions { absolute_tolerance: 0.0, relative_tolerance: 0.0, ..ComparisonOptions::default() }
        )?
        .equivalent
    );
    Ok(())
}

#[test]
fn compact_pointer_quotient_boundaries_have_independent_wire_examples() -> Result<()> {
    // Explicit wire words, not produced by the shared encoder or profile table.
    let probes: [(u32, &[i16]); 8] = [
        (0, &[1]),
        (32_766, &[32_767]),
        (32_767, &[-1, 1]),
        (32_768, &[-2, 1]),
        (65_533, &[-32_767, 1]),
        (65_534, &[-1, 2]),
        (65_535, &[-2, 2]),
        (1_073_709_055, &[-32_767, 32_767]),
    ];
    let mut text = b"8 0 0 ".to_vec();
    let mut binary = [8_i32.to_be_bytes(), 0_i32.to_be_bytes()].concat();
    binary.extend(1_i16.to_be_bytes()); // Null owner pointer.
    let mut expected = Vec::new();
    for (value, words) in probes {
        text.extend(format!("{value} ").as_bytes());
        for word in words {
            binary.extend(word.to_be_bytes());
        }
        expected.push(FieldValue::PointerIndex(value));
    }
    check_pair(74, 8, &text, &binary, 3, &expected)
}

#[test]
fn empty_and_long_utf16_arrays_preserve_units_and_boundaries() -> Result<()> {
    let units = [0_u16, 0x7FFF, 0x8000, 0x8004, 0x9F8D, 0xD83D, 0xDE00, 0xFFFF];
    for length in [0, 8, 257] {
        let values: Vec<_> = units.into_iter().cycle().take(length).collect();
        let mut text = String::new();
        for value in &values {
            let signed = if *value > 0x7FFF { i32::from(*value) - 65_536 } else { i32::from(*value) };
            write!(text, "{signed} ")?;
        }
        let binary: Vec<_> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
        let expected: Vec<_> = values.into_iter().map(FieldValue::UnicodeCharacter).collect();
        check_pair(98, i32::try_from(length)?, text.as_bytes(), &binary, 0, &expected)?;
    }
    Ok(())
}

#[test]
fn empty_arrays_and_null_numeric_values_are_distinct() -> Result<()> {
    for kind in [82, 83] {
        check_pair(kind, 0, b"", b"", 0, &[])?;
    }
    let integers = [i32::MIN, -32_764, 0, i32::MAX];
    let binary: Vec<_> = integers.into_iter().flat_map(i32::to_be_bytes).collect();
    check_pair(
        82,
        4,
        b"-2147483648 ?0 2147483647 ",
        &binary,
        0,
        &[FieldValue::Integer(Some(i32::MIN)), FieldValue::Integer(None), FieldValue::Integer(Some(0)), FieldValue::Integer(Some(i32::MAX))],
    )?;
    let binary: Vec<_> = [-3.14158e13_f64, 0.0, -0.125, 1.25e20].into_iter().flat_map(f64::to_be_bytes).collect();
    check_pair(
        83,
        4,
        b"?0 -0.125 1.25e20 ",
        &binary,
        0,
        &[FieldValue::Double(None), FieldValue::Double(Some(0.0)), FieldValue::Double(Some(-0.125)), FieldValue::Double(Some(1.25e20))],
    )
}
