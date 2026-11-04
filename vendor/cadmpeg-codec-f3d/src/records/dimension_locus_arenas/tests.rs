use super::{DesignDimensionLocusPairs, DesignDimensionNullLocusPairs};
use crate::native::F3dNative;
use crate::records::dimensions::DesignDimensionLocusPair;

fn pair(null_first: bool) -> DesignDimensionLocusPair {
    let shared = r#""id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80"#;
    let (opaque, first) = if null_first {
        ("", 0)
    } else {
        (r#","opaque_index":4,"opaque_index_offset":45"#, 40)
    };
    let first_offset = if null_first { 35 } else { 50 };
    let first_role_offset = first_offset + 10;
    let second_offset = first_offset + 15;
    let second_role_offset = second_offset + 10;
    let wire = format!(
        r#"{{{shared}{opaque},"first_geometry_record_index":{first},"first_geometry_reference_offset":{first_offset},"first_role":0,"first_role_offset":{first_role_offset},"second_geometry_record_index":41,"second_geometry_reference_offset":{second_offset},"second_role":1,"second_role_offset":{second_role_offset},"paired_class_tag":"273","paired_byte_offset":90}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn nonnull_arena_rejects_null_form_at_construction_and_deserialization() {
    let null_pair = pair(true);
    assert!(DesignDimensionLocusPairs::try_from(vec![null_pair.clone()]).is_err());
    let entry = serde_json::to_string(&null_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_locus_pairs":[{entry}]}}"#);
    let error = serde_json::from_str::<F3dNative>(&wire).unwrap_err();
    assert!(error.to_string().contains("design_dimension_locus_pairs"));
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_dimension_locus_pairs",
            &[null_pair],
        )
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());
}

#[test]
fn null_arena_rejects_nonnull_form_at_construction_and_deserialization() {
    let nonnull_pair = pair(false);
    assert!(DesignDimensionNullLocusPairs::try_from(vec![nonnull_pair.clone()]).is_err());
    let entry = serde_json::to_string(&nonnull_pair).unwrap();
    let wire = format!(r#"{{"design_dimension_null_locus_pairs":[{entry}]}}"#);
    assert!(serde_json::from_str::<F3dNative>(&wire).is_err());
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_dimension_null_locus_pairs",
            &[nonnull_pair],
        )
        .unwrap();
    assert!(F3dNative::load(&namespace).is_err());
}

#[test]
fn nonnull_arena_preserves_pair_wire() {
    let pair = pair(false);
    let entry = serde_json::to_string(&pair).unwrap();
    let native = F3dNative {
        design_dimension_locus_pairs: vec![pair].try_into().unwrap(),
        ..F3dNative::default()
    };
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(encoded.contains(&format!(r#""design_dimension_locus_pairs":[{entry}]"#)));
    assert_eq!(serde_json::from_str::<F3dNative>(&encoded).unwrap(), native);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let arena: Vec<DesignDimensionLocusPair> =
        namespace.arena_as("design_dimension_locus_pairs").unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), format!("[{entry}]"));
    assert_eq!(F3dNative::load(&namespace).unwrap(), native);
}

#[test]
fn null_locus_borrowed_wire_matches_owned_wire_bytes() {
    let pair = pair(true);
    let owned = crate::records::dimension_null_locus_wire::Wire::from(&pair);
    let borrowed = crate::records::dimension_null_locus_wire::BorrowedWire::from(&pair);
    assert_eq!(
        serde_json::to_vec(&borrowed).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}
