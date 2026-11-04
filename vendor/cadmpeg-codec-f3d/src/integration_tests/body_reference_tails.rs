// SPDX-License-Identifier: Apache-2.0
use super::decode;
use crate::test_support::body_reference_tail_test::{archive, frame, metadata};
use crate::test_support::native_test::{f3d_native, update_f3d_native};

#[test]
fn body_reference_tail_replays_exactly_and_unsupported_edits_are_refused() {
    let (bulk, frame) = frame(true, 9, 0);
    let source = archive(&bulk, &metadata(frame, 1));
    let decoded = decode(source.clone());
    let tails = f3d_native(decoded.ir()).design_body_reference_tails;
    assert_eq!(tails.len(), 1);
    assert_eq!(tails[0].serialization_form.value, 0);
    assert!(tails[0].first_references.is_empty());
    assert_eq!(tails[0].second_references[0].target(), Some(u64::MAX));
    assert_eq!(tails[0].trailing_form.value, 9);
    let mut output = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut output)
        .expect("unchanged reference tail replays");
    assert_eq!(output, source);
    let mut edited = decoded.ir().clone();
    update_f3d_native(&mut edited, |native| {
        native.design_body_reference_tails[0].trailing_form.value = 8;
    });
    let mut refused_output = Vec::new();
    let error = crate::test_support::plan_inherited_write(
        &edited,
        decoded.source_fidelity(),
        &mut refused_output,
    )
    .expect_err("unsupported native tail edit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(ref message)
        if message.contains("modified F3D IR contains edits beyond supported")),
        "{error}"
    );
    assert!(refused_output.is_empty());
}
