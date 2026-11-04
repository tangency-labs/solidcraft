// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::WritePath;

use crate::test_support::act_links::f3d_with_act_component_links;
use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::F3dCodec;

#[test]
fn component_link_root_counts_are_findings_after_a_complete_decode() {
    for links in [vec![(4, 0)], vec![(3, 0), (3, 1)], vec![(3, 1), (4, 0)]] {
        let source = f3d_with_act_component_links(&links);
        let decoded = F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert!(!decoded.ir().model.bodies.is_empty());
        let native = f3d_native(decoded.ir());
        assert_eq!(native.act_component_links.len(), links.len());
        for (link, &(tracked, _)) in native.act_component_links.iter().zip(&links) {
            assert_eq!(link.references.tracked_entity, tracked);
            assert_eq!(link.layout().entity_id(), format!("0_{tracked}"));
            assert_eq!(link.layout().display_name(), "Component");
        }
        let findings = crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, decoded.ir()).unwrap()
        });
        let root_findings = findings
            .iter()
            .filter(|finding| {
                finding.message
                    == "Fusion ACT stream does not have exactly one document-root component link"
            })
            .count();
        let roots = links.iter().filter(|&&(tracked, _)| tracked == 3).count();
        assert_eq!(root_findings, usize::from(roots != 1));
        let (ir, _, fidelity) = decoded.into_parts();
        let plan = F3dCodec
            .plan(
                EncodeInput::new(&ir, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .unwrap();
        let mut replay = Vec::new();
        plan.write_to(&mut replay).unwrap();
        assert_eq!(replay, source);
        let mut step = Vec::new();
        cadmpeg_codec_step::StepCodec::default()
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("step:ap242-e3"),
            )
            .unwrap()
            .write_to(&mut step)
            .unwrap();
        assert!(step.starts_with(b"ISO-10303-21;"));
    }
}

#[test]
fn non_root_component_link_references_patch_and_decode_without_coercion() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_act_component_links(&[(4, 0)])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (mut ir, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut ir, |native| {
        let link = &mut native.act_component_links[0];
        link.references.tracked_entity = 5;
        link.references.instance_root = 71;
        link.references.components_root = 72;
        link.try_set_strings("0_5".into(), "Renamed!!".into())
            .unwrap();
    });
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    assert!(matches!(report.write_path(), WritePath::Patched { .. }));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = f3d_native(decoded.ir());
    let link = &native.act_component_links[0];
    assert_eq!(link.references.tracked_entity, 5);
    assert_eq!(link.references.instance_root, 71);
    assert_eq!(link.references.components_root, 72);
    assert_eq!(link.layout().display_name(), "Renamed!!");
}
