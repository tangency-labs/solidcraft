//! Public-reference V13 LIST bytes cover fields hidden by the V30 End opcode.
#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, BuiltinSchemaProfile, ComparisonOptions, DocumentLimits, FieldValue, SchemaKey, SchemaProvider, SchemaTypeLookup,
    compare_xt_xb_documents, parse_xb, parse_xt,
    schema::profiles::{icad_sch30000_13006, onshape_sch13006},
};
use support::{Result, pointer, terminate, xb_header, xt_header};

#[test]
fn base_profiles_have_independent_hashes_and_exact_scope() -> Result<()> {
    for profile in [onshape_sch13006()?, icad_sch30000_13006()?] {
        let standard = profile.metadata().profile_id == "onshape-sch13006-r6";
        assert_eq!(profile.definitions().len(), if standard { 40 } else { 43 });
        assert_eq!(profile.definitions().map(|d| d.fields.len()).sum::<usize>(), if standard { 327 } else { 366 });
        assert_eq!(profile.metadata().profile_sha256, support::profile_hash(&profile)?);
        assert_eq!(profile.definition(12).ok_or("missing BODY")?.fields.len(), 23);
        assert_eq!(profile.definition(70).ok_or("missing LIST")?.fields.len(), 12);
        assert_eq!(profile.definition(74).ok_or("missing list block")?.fields.len(), 3);
        assert_eq!(profile.definition(19).ok_or("missing REGION")?.fields.len(), 7);
    }
    let registry = BuiltinProfileRegistry::new(vec![onshape_sch13006()?, icad_sch30000_13006()?])?;
    let key = SchemaKey::parse("SCH_3000310_30000_13006")?;
    let provider = registry.provider_for_key(&key).ok_or("missing provider")?;
    assert_eq!(provider.lookup_type("13006", 110), SchemaTypeLookup::Unknown);
    assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
    assert_eq!(provider.lookup_type("30000", 204), SchemaTypeLookup::Unknown);
    for node_type in [185, 203, 205] {
        assert_eq!(provider.lookup_type("13006", node_type), SchemaTypeLookup::Unknown);
    }
    assert_eq!(icad_sch30000_13006()?.absent_base_types().collect::<Vec<_>>(), [204]);
    // The embedded profile reuses these declarations for the types seen under
    // its key; blend boundaries and spun surfaces were not seen there.
    for kind in [45, 124, 125, 126, 127, 128, 134, 135, 136, 137] {
        assert!(onshape_sch13006()?.definition(kind).is_some());
        assert_eq!(onshape_sch13006()?.definition(kind), icad_sch30000_13006()?.definition(kind));
    }
    for kind in [54, 56, 60] {
        assert!(icad_sch30000_13006()?.definition(kind).is_some());
    }
    for kind in [59, 68] {
        assert!(icad_sch30000_13006()?.definition(kind).is_none());
    }
    assert!(onshape_sch13006()?.definition(133).is_some());
    assert_eq!(icad_sch30000_13006()?.definition(133).ok_or("missing trim")?.fields.len(), 12);
    assert!(onshape_sch13006()?.absent_base_types().next().is_none());
    assert!(icad_sch30000_13006()?.definition(204).is_none());
    assert!(registry.provider_for_key(&SchemaKey::parse("SCH_3001232_30100_13006")?).is_none());
    Ok(())
}

#[test]
fn embedded_membership_is_part_of_the_profile_digest() -> Result<()> {
    let original = icad_sch30000_13006()?;
    let digest = support::profile_hash(&original)?;
    let mut classified = Vec::new();
    for (unsupported, absent) in [(vec![110], vec![]), (vec![], vec![110])] {
        let profile = BuiltinSchemaProfile::new_embedded(
            original.metadata().clone(),
            original.accepted_schema_keys().map(|key| key.raw().to_owned()).collect(),
            original.definitions().cloned().collect(),
            unsupported,
            absent,
        )?;
        classified.push(support::profile_hash(&profile)?);
    }
    assert_ne!(classified[0], digest);
    assert_ne!(classified[1], digest);
    assert_ne!(classified[0], classified[1]);
    Ok(())
}

#[test]
fn v13_list_retains_its_complete_tail() -> Result<()> {
    let key = "SCH_1300000_13006";
    let registry = BuiltinProfileRegistry::new(vec![onshape_sch13006()?])?;
    let parsed_key = SchemaKey::parse(key)?;
    let provider = registry.provider_for_key(&parsed_key).ok_or("missing provider")?;
    let mut xt = xt_header(key, 0);
    xt.extend(b"70 1 0 0 0 0 4 2 20 8 0 0 1 T1 0 ");
    let mut xb = xb_header(key, 0)?;
    xb.extend(70_u16.to_be_bytes());
    xb.extend(pointer(1)?);
    xb.extend(0_i32.to_be_bytes());
    xb.extend(pointer(0)?.repeat(3));
    for value in [4_i32, 2, 20, 8] {
        xb.extend(value.to_be_bytes());
    }
    xb.extend(pointer(0)?.repeat(2));
    xb.extend(1_i32.to_be_bytes());
    xb.push(1);
    terminate(&mut xb);
    let a = parse_xt(&xt, &provider, DocumentLimits::default())?;
    let b = parse_xb(&xb, &provider, DocumentLimits::default())?;
    assert!(compare_xt_xb_documents(&a, &b, ComparisonOptions::default())?.equivalent);
    assert_eq!(b.nodes[0].fields.len(), 12);
    assert_eq!(b.nodes[0].fields[9].values, [FieldValue::PointerIndex(0)]);
    assert_eq!(b.nodes[0].fields[10].values, [FieldValue::Integer(Some(1))]);
    assert_eq!(b.nodes[0].fields[11].values, [FieldValue::Logical(true)]);
    Ok(())
}
