// SPDX-License-Identifier: Apache-2.0
use crate::validate::{validate_body_bindings, Ctx};
use cadmpeg_ir::{report::check::Check, CadIr};

#[test]
fn repro_fragment_skips_external_body_keys_and_checks_local_binding_frames() {
    let binding = serde_json::from_value(serde_json::json!({
        "id":"f3d:Design1/BulkStream.dat:design-body-binding#16",
        "stream":"Design1/BulkStream.dat", "pair_count":1, "pair_ordinal":0,
        "asm_body_key":1, "asm_body_key_offset":16,
        "entity_suffix":7, "entity_suffix_offset":24,
        "blob_name":"BREP.synthetic.smb", "blob_name_offset":32,
        "body":"f3d:brep:entity#1"
    }))
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_body_bindings.push(binding);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let mut document = CadIr::empty();
    let mut findings = Vec::new();
    validate_body_bindings(&Ctx::new(&document, &native, &ctx), &mut findings).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
    document.fragment = true;
    findings.clear();
    validate_body_bindings(&Ctx::new(&document, &native, &ctx), &mut findings).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.check == Check::FragmentSkipped));
    assert!(findings
        .iter()
        .all(|finding| finding.check != Check::NativeLinks));
    native
        .design_body_bindings
        .push(native.design_body_bindings[0].clone());
    findings.clear();
    validate_body_bindings(&Ctx::new(&document, &native, &ctx), &mut findings).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn repro_fragment_native_reader_skips_refused_rows_and_keeps_admitted_records() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    namespace.set_arena(&ctx, "design_record_headers", &[
        serde_json::json!({"id":"f3d:design:design-record-header#1", "byte_offset":1, "class_tag":"257", "record_index":1}),
        serde_json::json!({"id":"f3d:design:design-record-header#2"}),
    ]).unwrap();
    assert!(crate::native::F3dNative::load_inner(&ctx, &namespace).is_err());
    let (native, skipped) = crate::native::F3dNative::load_fragment(&ctx, &namespace).unwrap();
    assert_eq!(native.design_record_headers.len(), 1);
    assert_eq!(native.design_record_headers[0].record_index, 1);
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].starts_with("design_record_headers:"));
}
