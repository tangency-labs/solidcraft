// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn repro_streams_keep_member_grammar_and_share_anonymous_owner() {
    let mut owners = BTreeMap::new();
    let mut replacements = BTreeMap::new();
    remember_scope(
        "PLANTED_SOURCE/Design1/BulkStream.dat",
        &mut owners,
        &mut replacements,
    );
    remember_scope(
        "PLANTED_SOURCE/Design1/MetaStream.dat",
        &mut owners,
        &mut replacements,
    );
    assert_eq!(
        replacements["PLANTED_SOURCE/Design1/BulkStream.dat"],
        "repro0/repro1/BulkStream.dat"
    );
    assert_eq!(
        replacements["PLANTED_SOURCE/Design1/MetaStream.dat"],
        "repro0/repro1/MetaStream.dat"
    );
    assert!(replacements
        .values()
        .all(|value| !value.contains("PLANTED")));
    assert!(crate::ids::native_scope_matches(
        "f3d:repro0/repro1/BulkStream.dat",
        "repro0/repro1/BulkStream.dat"
    ));
}

#[test]
fn repro_body_binding_accepts_anonymous_stream_and_blob() {
    let original = serde_json::json!({
        "id":"f3d:PLANTED_SOURCE/Design1/BulkStream.dat:design-body-binding#16",
        "stream":"PLANTED_SOURCE/Design1/BulkStream.dat", "pair_count":1,
        "pair_ordinal":0, "asm_body_key":1, "asm_body_key_offset":16,
        "entity_suffix":7, "entity_suffix_offset":24,
        "blob_name":"BREP.PLANTED_FILE.smb", "blob_name_offset":32,
        "body":null
    });
    let mut original = original;
    original.as_object_mut().unwrap().remove("body");
    let _: crate::records::bodies::DesignBodyBinding =
        serde_json::from_value(original.clone()).unwrap();
    let mut document = CadIr::empty();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    document
        .native
        .namespace_mut("f3d")
        .set_arena(&ctx, "design_body_bindings", &[original.clone()])
        .unwrap();
    let replacements = redactions(&document).unwrap();
    let anonymous = replacements["PLANTED_SOURCE/Design1/BulkStream.dat"].clone();
    original["stream"] = serde_json::json!(anonymous);
    original["id"] = serde_json::json!(format!("f3d:{anonymous}:design-body-binding#16"));
    original["blob_name"] = serde_json::json!(replacements["BREP.PLANTED_FILE.smb"]);
    let _: crate::records::bodies::DesignBodyBinding =
        serde_json::from_value(original.clone()).unwrap();
    assert!(!original.to_string().contains("PLANTED"));
}

#[test]
fn repro_xref_stream_keeps_occurrence_and_member_joins() {
    let mut original = serde_json::json!({
        "id":"f3d:xref/PLANTED_CONTEXT/occurrence-7/PLANTED_SOURCE/Design1/BulkStream.dat:design-body-binding#16",
        "stream":"PLANTED_SOURCE/Design1/BulkStream.dat", "pair_count":1,
        "pair_ordinal":0, "asm_body_key":1, "asm_body_key_offset":16,
        "entity_suffix":7, "entity_suffix_offset":24,
        "blob_name":"BREP.PLANTED_FILE.smb", "blob_name_offset":32
    });
    let _: crate::records::bodies::DesignBodyBinding =
        serde_json::from_value(original.clone()).unwrap();
    let mut document = CadIr::empty();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    document
        .native
        .namespace_mut("f3d")
        .set_arena(&ctx, "design_body_bindings", &[original.clone()])
        .unwrap();
    let replacements = redactions(&document).unwrap();
    original["stream"] = serde_json::json!(replacements["PLANTED_SOURCE/Design1/BulkStream.dat"]);
    original["id"] = serde_json::json!(format!(
        "{}:design-body-binding#16",
        replacements["f3d:xref/PLANTED_CONTEXT/occurrence-7/PLANTED_SOURCE/Design1/BulkStream.dat"]
    ));
    original["blob_name"] = serde_json::json!(replacements["BREP.PLANTED_FILE.smb"]);
    let _: crate::records::bodies::DesignBodyBinding =
        serde_json::from_value(original.clone()).unwrap();
    assert!(original["id"].as_str().unwrap().contains("/occurrence-7/"));
    assert!(!original.to_string().contains("PLANTED"));
}
