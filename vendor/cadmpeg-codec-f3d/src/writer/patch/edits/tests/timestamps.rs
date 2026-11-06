// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::records::recipes::CreationTimestamp;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

#[test]
fn timestamp_patch_keeps_identity_after_native_storage_sorts_records() {
    let mut source = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = source.model.bodies[0].id.clone();
    let face = source.model.faces[0].id.clone();
    f3d_native_mut(&mut source).creation_timestamps = vec![
        CreationTimestamp {
            id: "f3d:generated:creation-timestamp#0".into(),
            target: AttributeTarget::Body(body),
            record_index: 0,
            unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1_579_392_000_000_001.0)
                .unwrap(),
        },
        CreationTimestamp {
            id: "f3d:generated:creation-timestamp#1".into(),
            target: AttributeTarget::Face(face),
            record_index: 0,
            unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1_579_392_000_000_002.0)
                .unwrap(),
        },
    ];
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("encode two timestamp carriers");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the generated carriers");
    let (mut target, _, fidelity) = decoded.into_parts();
    let expected = {
        let mut native = f3d_native_mut(&mut target);
        assert_eq!(native.creation_timestamps.len(), 2);
        native.creation_timestamps.sort_by(|a, b| b.id.cmp(&a.id));
        let previous = native.creation_timestamps[0].unix_microseconds.get();
        native.creation_timestamps[0].unix_microseconds =
            cadmpeg_ir::scalar::FiniteReal::new(previous + 100.0).unwrap();
        native
            .creation_timestamps
            .iter()
            .map(|timestamp| (timestamp.id.clone(), timestamp.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &target).expect("validate the native namespace")
    })
    .is_empty());
    let mut unordered = target.clone();
    unordered
        .native
        .namespace_mut("f3d")
        .arenas_mut()
        .get_mut("creation_timestamps")
        .unwrap()
        .reverse();
    let unordered: cadmpeg_ir::CadIr =
        serde_json::from_value(serde_json::to_value(&unordered).unwrap()).unwrap();
    assert!(
        cadmpeg_ir::validate::validate_neutral(&unordered, Vec::new())
            .expect("validate the neutral model")
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::ArenaOrder)
    );
    let error = F3dCodec
        .plan(
            EncodeInput::new(&unordered, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("timestamp patch rejects the unsorted arena");
    assert!(error.to_string().contains("unchanged timestamp-id set"));
    let mut patched = Vec::new();
    let report = F3dCodec
        .plan(
            EncodeInput::new(&target, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut patched))
        .expect("patch timestamps after record-list reordering");
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));
    let result = F3dCodec
        .decode(&mut Cursor::new(patched), &DecodeOptions::default())
        .expect("decode patched timestamp carriers");
    let actual = f3d_native(result.ir())
        .creation_timestamps
        .into_iter()
        .map(|timestamp| (timestamp.id.clone(), timestamp))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual, expected);
}
