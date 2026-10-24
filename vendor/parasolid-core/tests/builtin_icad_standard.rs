//! Authored checks for the exact non-embedded iCAD keys. Field codes, new
//! reference classes and record values are stated here independently of the
//! profile table; real inputs are compared separately against a catalog.

#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, BuiltinSchemaProfile, DocumentLimits, ErrorKind, SchemaKey, SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::{icad_legacy_standard, onshape_sch13006, onshape_sch30000},
};
use serde_json::{Value, json};
use support::Result;

struct Expected {
    key: &'static str,
    types: usize,
    fields: usize,
    body: &'static str,
    /// Ordinal and pointer class of each BODY field that the base lacks.
    body_references: &'static [(usize, u16)],
    region_head: usize,
    region: &'static str,
    intersection: Option<&'static str>,
    limit: Option<&'static str>,
    list: &'static str,
    block: &'static str,
}

const EXPECTED: [Expected; 11] = [
    Expected {
        key: "SCH_1300218_13006",
        types: 23,
        fields: 189,
        body: "dppppppffpppupuuppppppp",
        body_references: &[],
        region_head: 20,
        region: "dpppppc",
        intersection: Some("dpppppcpppp"),
        limit: Some("ch"),
        list: "dpppddddppdl",
        block: "dpp",
    },
    Expected {
        key: "SCH_1302234_13006",
        types: 19,
        fields: 165,
        body: "dppppppffpppupuuppppppp",
        body_references: &[],
        region_head: 20,
        region: "dpppppc",
        intersection: None,
        limit: None,
        list: "dpppddddppdl",
        block: "dpp",
    },
    Expected {
        key: "SCH_1500000_15003",
        types: 28,
        fields: 242,
        body: "dppppppffpppupuuppppppp",
        body_references: &[],
        region_head: 20,
        region: "dpppppc",
        intersection: Some("dpppppcpppp"),
        limit: Some("ch"),
        list: "dulpppdddpp",
        block: "dpp",
    },
    Expected {
        key: "SCH_1700000_16100",
        types: 20,
        fields: 176,
        body: "dppppppffpppupuuppppppp",
        body_references: &[],
        region_head: 20,
        region: "dpppppc",
        intersection: None,
        limit: None,
        list: "dulpppdddpp",
        block: "dpp",
    },
    Expected {
        key: "SCH_1901000_19008",
        types: 20,
        fields: 181,
        body: "dppppppffpppupuupppppppdppp",
        body_references: &[(24, 82), (25, 82), (26, 82)],
        region_head: 20,
        region: "dpppppc",
        intersection: None,
        limit: None,
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_2401000_20000",
        types: 19,
        fields: 169,
        body: "dppppppffpppupuupppppppdppp",
        body_references: &[(24, 82), (25, 82), (26, 82)],
        region_head: 20,
        region: "dpppppc",
        intersection: None,
        limit: None,
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_2601000_26105",
        types: 38,
        fields: 336,
        body: "dppppppffpppupuuppppppppdppppd",
        body_references: &[(13, 1040), (23, 1006), (25, 82), (26, 82), (27, 82), (28, 12)],
        region_head: 20,
        region: "dpppppcp",
        intersection: Some("dpppppcpppp"),
        limit: Some("ch"),
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_2800000_28002",
        types: 41,
        fields: 370,
        body: "dppppppffpppupuuppppppppdppppd",
        body_references: &[(13, 1040), (23, 1006), (25, 82), (26, 82), (27, 82), (28, 12)],
        region_head: 20,
        region: "dpppppcp",
        intersection: Some("dpppppcpppp"),
        limit: Some("cch"),
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_2901000_28101",
        types: 24,
        fields: 214,
        body: "dppppppppffpppupuupppppppppdppppd",
        body_references: &[(6, 1006), (7, 1008), (15, 1040), (22, 1006), (23, 1008), (28, 82), (29, 82), (30, 82), (31, 12)],
        region_head: 24,
        region: "dpppppcp",
        intersection: Some("dpppppcpppp"),
        limit: Some("cch"),
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_3200000_32001",
        types: 30,
        fields: 255,
        body: "dppppppppffpppupuupppppppppdppppdp",
        body_references: &[(6, 1006), (7, 1008), (15, 1040), (22, 1006), (23, 1008), (28, 82), (29, 82), (30, 82), (31, 12), (33, 206)],
        region_head: 24,
        region: "dpppppcp",
        intersection: Some("dpppppcppppp"),
        limit: Some("cch"),
        list: "dulpppdddpp",
        block: "ddpp",
    },
    Expected {
        key: "SCH_3301000_33103",
        types: 25,
        fields: 219,
        body: "dpppppppppffpppupuupppppppppdppppdp",
        body_references: &[(3, 222), (7, 1006), (8, 1008), (16, 1040), (23, 1006), (24, 1008), (29, 82), (30, 82), (31, 82), (32, 12), (34, 206)],
        region_head: 25,
        region: "dpppppcp",
        intersection: Some("dpppppcppppp"),
        limit: Some("cch"),
        list: "dulpppdddpp",
        block: "ddpp",
    },
];

fn codes(profile: &BuiltinSchemaProfile, node_type: u16) -> Option<String> {
    profile.definition(node_type).map(|definition| definition.fields.iter().map(|field| field.field_type.code()).collect())
}

#[test]
fn exact_profiles_and_canonical_hashes() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = icad_legacy_standard()?;
    assert_eq!(profiles.len(), EXPECTED.len());
    for (profile, expected) in profiles.iter().zip(&EXPECTED) {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        assert_eq!(key.raw(), expected.key);
        assert_eq!(profile.accepted_schema_keys().len(), 1);
        assert_eq!(profile.metadata().revision, 1);
        assert_eq!(profile.metadata().profile_id, format!("icad-{}-r1", expected.key[4..].replace('_', "-")));
        assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
        assert_eq!(profile.definitions().len(), expected.types);
        assert_eq!(profile.definitions().map(|d| d.fields.len()).sum::<usize>(), expected.fields);
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        let schema = key.provider_schema();
        for kind in [55, 57, 58, 67, 69, 110, 138] {
            assert_eq!(provider.lookup_type(schema, kind), SchemaTypeLookup::Unknown);
        }
        assert_eq!(provider.lookup_type("99999", 29), SchemaTypeLookup::Unknown);
        for component in 1..=2 {
            let mut parts = key.raw().split('_').map(str::to_owned).collect::<Vec<_>>();
            parts[component].push('9');
            let nearby = SchemaKey::parse(&parts.join("_"))?;
            assert!(!provider.supports_schema_key(&nearby));
            assert!(registry.provider_for_key(&nearby).is_none());
        }
        let embedded = SchemaKey::parse(&format!("{}_13006", key.raw()))?;
        assert!(!provider.supports_schema_key(&embedded));
    }
    Ok(())
}

#[test]
fn revision_layouts_have_the_stated_codecs_and_references() -> Result<()> {
    for (profile, expected) in icad_legacy_standard()?.iter().zip(&EXPECTED) {
        let body = profile.definition(12).ok_or("body")?;
        assert_eq!(codes(profile, 12).as_deref(), Some(expected.body));
        for (ordinal, class) in expected.body_references {
            assert_eq!(body.fields[*ordinal].pointer_class, *class);
        }
        // The mapped references keep their reviewed names at the stated place.
        for (offset, name) in ["region_head", "edge_head", "vertex_head"].into_iter().enumerate() {
            assert_eq!(body.fields[expected.region_head + offset].name, name);
            assert_eq!(body.fields.iter().filter(|f| f.name == name).count(), 1);
        }
        assert_eq!(codes(profile, 19).as_deref(), Some(expected.region));
        assert_eq!(codes(profile, 38).as_deref(), expected.intersection);
        assert_eq!(codes(profile, 41).as_deref(), expected.limit);
        assert_eq!(codes(profile, 70).as_deref(), Some(expected.list));
        assert_eq!(codes(profile, 74).as_deref(), Some(expected.block));
        assert!(profile.definition(74).is_some_and(|d| d.variable));
        if let Some(intersection) = profile.definition(38) {
            let data = intersection.fields.iter().filter(|f| f.name == "intersection_data").collect::<Vec<_>>();
            assert_eq!(data.len(), usize::from(intersection.fields.len() == 12));
            assert!(data.iter().all(|f| f.pointer_class == 204));
            assert_eq!(profile.definition(204).is_some(), !data.is_empty());
        }
    }
    Ok(())
}

#[test]
fn layouts_agree_with_the_reviewed_v13_and_v30_profiles() -> Result<()> {
    let v13 = onshape_sch13006()?;
    let v30 = onshape_sch30000()?;
    let layout = |profile: &BuiltinSchemaProfile, kind: u16| {
        profile.definition(kind).map(|d| d.fields.iter().map(|f| (f.field_type.code(), f.pointer_class, f.element_count)).collect::<Vec<_>>())
    };
    for (profile, expected) in icad_legacy_standard()?.iter().zip(&EXPECTED) {
        let schema = profile.metadata().provider_schema.as_str();
        // A 13006 key is the base itself: every listed type is unchanged.
        if schema == "13006" {
            for definition in profile.definitions() {
                assert_eq!(Some(definition), v13.definition(definition.node_type));
            }
        }
        // BODY has the V30 layout from 28101 until a later reference is added.
        if schema == "28101" {
            assert_eq!(profile.definition(12).map(|d| &d.fields), v30.definition(12).map(|d| &d.fields));
        }
        for (kind, changed) in
            [(19, expected.region.len() == 8), (41, expected.limit == Some("cch")), (38, expected.intersection.is_some_and(|c| c.len() == 12))]
        {
            if changed {
                assert_eq!(profile.definition(kind).map(|d| &d.fields), v30.definition(kind).map(|d| &d.fields));
            } else if profile.definition(kind).is_some() {
                assert_eq!(profile.definition(kind).map(|d| &d.fields), v13.definition(kind).map(|d| &d.fields));
            }
        }
        if profile.definition(204).is_some() {
            assert_eq!(layout(profile, 204), layout(&v30, 204));
        }
        for (kind, changed) in [(70, expected.list.len() == 11), (74, expected.block.len() == 4)] {
            let reviewed = if changed { &v30 } else { &v13 };
            assert_eq!(layout(profile, kind), layout(reviewed, kind));
        }
    }
    Ok(())
}

/// Encode one record from its field codes; every ordinal has a distinct value.
fn record(node_type: u16, codes: &str, variable: Option<&[u32]>) -> Result<(Vec<u8>, Value)> {
    let mut binary = node_type.to_be_bytes().to_vec();
    if let Some(entries) = variable {
        binary.extend(i32::try_from(entries.len())?.to_be_bytes());
    }
    binary.extend(support::pointer(1)?);
    let mut expected = Vec::new();
    let fixed = codes.len() - usize::from(variable.is_some());
    for (ordinal, code) in codes.bytes().take(fixed).enumerate() {
        let number = u32::try_from(ordinal)?;
        match code {
            b'd' => {
                let value = 1000 + i32::try_from(ordinal)?;
                binary.extend(value.to_be_bytes());
                expected.push(json!([value]));
            }
            b'p' => {
                binary.extend(support::pointer(number + 2)?);
                expected.push(json!([number + 2]));
            }
            b'f' => {
                let value = f64::from(number) + 0.25;
                binary.extend(value.to_be_bytes());
                expected.push(json!([value]));
            }
            b'u' => {
                binary.push(u8::try_from(number + 3)?);
                expected.push(json!([number + 3]));
            }
            b'l' => {
                binary.push(1);
                expected.push(json!([true]));
            }
            b'c' => {
                binary.push(b'A' + u8::try_from(ordinal)?);
                expected.push(json!([b'A' + u8::try_from(ordinal)?]));
            }
            _ => return Err("unexpected authored code".into()),
        }
    }
    if let Some(entries) = variable {
        for entry in entries {
            binary.extend(support::pointer(*entry)?);
        }
        expected.push(json!(entries));
    }
    Ok((binary, json!(expected)))
}

#[test]
fn authored_records_decode_with_each_revision_layout() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for expected in &EXPECTED {
        let key = SchemaKey::parse(expected.key)?;
        let provider = registry.provider_for_key(&key).ok_or("provider")?;
        let mut records = vec![
            record(12, expected.body, None)?,
            record(19, expected.region, None)?,
            record(70, expected.list, None)?,
            record(74, expected.block, Some(&[5, 6, 32_767]))?,
            record(74, expected.block, Some(&[]))?,
        ];
        if let Some(limit) = expected.limit {
            // An empty limit still carries its leading characters.
            let leading = &limit[..limit.len() - 1];
            let (mut binary, _) = record(41, leading, None)?;
            binary.splice(2..2, 0_i32.to_be_bytes());
            let mut values = (b'A'..).take(leading.len()).map(|c| json!([c])).collect::<Vec<_>>();
            values.push(json!([]));
            records.push((binary, json!(values)));
        }
        for (payload, values) in records {
            let mut binary = support::xb_header(expected.key, 0)?;
            binary.extend(&payload);
            support::terminate(&mut binary);
            let document = parse_xb(&binary, &provider, DocumentLimits::default())?;
            assert_eq!(document.nodes.len(), 1);
            assert_eq!(document.terminator.byte_range.end, binary.len());
            let decoded = document.nodes[0].fields.iter().map(|f| f.values.iter().map(support::value_json).collect::<Vec<_>>()).collect::<Vec<_>>();
            assert_eq!(json!(decoded), values);
            assert_eq!(support::encode_document_nodes(expected.key, &document.nodes)?, binary);
            for end in 0..binary.len() {
                assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn independent_point_values_roles_and_uncovered_types() -> Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for expected in &EXPECTED {
        let key = SchemaKey::parse(expected.key)?;
        let provider = registry.provider_for_key(&key).ok_or("provider")?;
        let mut binary = support::xb_header(expected.key, 0)?;
        binary.extend([0, 29]);
        binary.extend(support::pointer(7)?);
        binary.extend(123_i32.to_be_bytes());
        binary.extend(support::pointer(0)?.repeat(4));
        for value in [13.25_f64, -17.5, 23.75] {
            binary.extend(value.to_be_bytes());
        }
        support::terminate(&mut binary);
        let mut text = support::xt_header(expected.key, 0);
        text.extend(b"29 7 123 0 0 0 0 13.25 -17.5 23.75 1 0");
        let a = parse_xb(&binary, &provider, DocumentLimits::default())?;
        let b = parse_xt(&text, &provider, DocumentLimits::default())?;
        for (x, y) in a.nodes[0].fields.iter().zip(&b.nodes[0].fields) {
            assert_eq!(x.values, y.values);
        }
        assert_eq!(a.nodes[0].fields[5].values, [parasolid_core::FieldValue::Vector([Some(13.25), Some(-17.5), Some(23.75),])]);
        // The role gate passes; this deliberately isolated point has no body.
        assert_eq!(parasolid_core::brep::map_xb_brep(&a).err().ok_or("body")?.kind(), ErrorKind::MissingBrepBody);
        // A swept surface was not seen under any of these keys.
        let mut uncovered = support::xb_header(expected.key, 0)?;
        uncovered.extend([0, 67]);
        assert_eq!(
            parse_xb(&uncovered, &provider, DocumentLimits::default()).err().ok_or("uncovered type")?.kind(),
            ErrorKind::BuiltinProfileUncoveredType
        );
    }
    Ok(())
}
