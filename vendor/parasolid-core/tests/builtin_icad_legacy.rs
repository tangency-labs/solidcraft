#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, ErrorKind, SchemaKey, SchemaProvider, SchemaTypeLookup, parse_xb, parse_xt,
    schema::profiles::icad_legacy_13006,
};

// Stated independently of the profile table: exact key, revision, extra base
// types beyond the shared 31-type subset, and the type and field counts.
const EXPECTED: [(&str, u32, &[u16], usize, usize); 15] = [
    ("SCH_1500137_15003_13006", 1, &[], 31, 252),
    ("SCH_1500245_15003_13006", 1, &[56, 59, 68], 34, 292),
    ("SCH_1700223_16100_13006", 1, &[], 31, 252),
    ("SCH_1700256_16100_13006", 1, &[45, 68, 127, 128, 134, 135, 136, 137], 39, 305),
    ("SCH_1901261_19008_13006", 1, &[], 31, 252),
    ("SCH_1901315_19008_13006", 1, &[], 31, 252),
    ("SCH_2100293_20000_13006", 2, &[45, 56, 59, 60, 124, 125, 126, 127, 128, 134, 135, 136, 137], 44, 375),
    ("SCH_2100311_20000_13006", 1, &[45, 56, 59, 124, 125, 126, 127, 128, 134, 135, 136, 137], 43, 363),
    ("SCH_2401260_20000_13006", 1, &[], 31, 252),
    ("SCH_2601246_26105_13006", 1, &[45, 56, 60, 124, 125, 126, 127, 128, 134, 135, 136, 137], 43, 366),
    ("SCH_2800188_28002_13006", 1, &[], 31, 252),
    ("SCH_2901199_28101_13006", 1, &[45, 56, 59, 68, 124, 125, 126, 127, 128, 134, 135, 136], 43, 368),
    ("SCH_3200152_32001_13006", 1, &[45, 68, 127, 128, 134, 135, 136, 137], 39, 305),
    ("SCH_3200252_32001_13006", 1, &[45, 56, 59, 68, 124, 125, 126, 127, 128, 134, 135, 136, 137], 44, 379),
    ("SCH_3301231_33103_13006", 2, &[45, 56, 59, 60, 68, 124, 125, 126, 127, 128, 134, 135, 136, 137], 45, 391),
];

#[test]
fn exact_profiles_and_canonical_hashes() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    let profiles = icad_legacy_13006()?;
    assert_eq!(profiles.len(), EXPECTED.len());
    for (profile, (expected_key, revision, _, types, fields)) in profiles.iter().zip(EXPECTED) {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        assert_eq!(key.raw(), expected_key);
        assert_eq!(profile.metadata().revision, revision);
        assert_eq!(profile.metadata().profile_id, format!("icad-{}-r{revision}", expected_key[4..].replace('_', "-")));
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        assert_eq!(profile.metadata().profile_sha256, support::profile_hash(profile)?);
        assert_eq!(profile.definitions().len(), types);
        assert_eq!(profile.definitions().map(|d| d.fields.len()).sum::<usize>(), fields);
        assert_eq!(provider.lookup_type("13006", 204), SchemaTypeLookup::Absent);
        for kind in [55, 57, 58, 67, 69, 110, 138] {
            assert_eq!(provider.lookup_type("13006", kind), SchemaTypeLookup::Unknown);
        }
        for component in 1..=3 {
            let mut parts = key.raw().split('_').map(str::to_owned).collect::<Vec<_>>();
            parts[component].push('9');
            let nearby = SchemaKey::parse(&parts.join("_"))?;
            assert!(!provider.supports_schema_key(&nearby));
            assert!(registry.provider_for_key(&nearby).is_none());
        }
    }
    Ok(())
}

#[test]
fn extra_membership_is_scoped_to_observed_keys() -> support::Result<()> {
    for (profile, (_, _, extra, _, _)) in icad_legacy_13006()?.iter().zip(EXPECTED) {
        for kind in [45, 56, 59, 60, 68, 124, 125, 126, 127, 128, 134, 135, 136, 137] {
            assert_eq!(profile.definition(kind).is_some(), extra.contains(&kind));
        }
    }
    Ok(())
}

#[test]
fn independent_point_values_roles_truncation_and_unknown_membership() -> support::Result<()> {
    let registry = BuiltinProfileRegistry::compiled()?;
    for profile in icad_legacy_13006()? {
        let key = profile.accepted_schema_keys().next().ok_or("key")?;
        let provider = registry.provider_for_key(key).ok_or("provider")?;
        let mut binary = support::xb_header(key.raw(), 0)?;
        binary.extend([0, 29, 255]);
        binary.extend(support::pointer(7)?);
        binary.extend(123_i32.to_be_bytes());
        binary.extend(support::pointer(0)?.repeat(4));
        for value in [13.25_f64, -17.5, 23.75] {
            binary.extend(value.to_be_bytes());
        }
        support::terminate(&mut binary);
        let mut text = support::xt_header(key.raw(), 0);
        text.extend(b"29 255 7 123 0 0 0 0 13.25 -17.5 23.75 1 0");
        let a = parse_xb(&binary, &provider, DocumentLimits::default())?;
        let b = parse_xt(&text, &provider, DocumentLimits::default())?;
        for (x, y) in a.nodes[0].fields.iter().zip(&b.nodes[0].fields) {
            assert_eq!(x.values, y.values);
        }
        assert_eq!(a.nodes[0].fields[5].values, [parasolid_core::FieldValue::Vector([Some(13.25), Some(-17.5), Some(23.75),])]);
        assert_eq!(support::encode_document_nodes(key.raw(), &a.nodes)?, binary);
        // The role gate passes; this deliberately isolated point has no body.
        assert_eq!(parasolid_core::brep::map_xb_brep(&a).err().ok_or("body")?.kind(), ErrorKind::MissingBrepBody);
        for end in 0..binary.len() {
            assert!(parse_xb(&binary[..end], &provider, DocumentLimits::default()).is_err());
        }
        let mut unknown = support::xb_header(key.raw(), 0)?;
        unknown.extend([0, 67, 255]);
        let error = parse_xb(&unknown, &provider, DocumentLimits::default()).err().ok_or("unknown type")?;
        assert_eq!(error.kind(), ErrorKind::UnknownBaseSchemaType);
    }
    Ok(())
}
