// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native_mut, update_f3d_native};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;

#[test]
fn generated_source_less_rejects_act_without_segment_metadata() {
    use crate::records::act::ActEntity;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.act_entities = vec![ActEntity::try_new(
        "f3d:generated:act-entity#7".into(),
        7,
        "0_985".into(),
        Some(crate::records::act::ActTableRow::new(0).unwrap()),
        crate::records::act::ActChannelGroup::try_new(
            100,
            Some(200),
            "261".to_owned().try_into().unwrap(),
            std::collections::BTreeMap::from([(
                "Appearance".into(),
                crate::records::identity::Located {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 120,
                },
            )]),
            None,
        )
        .unwrap(),
    )
    .unwrap()];
    drop(native);
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("ACT generation without its record registry must fail atomically");
    assert!(error
        .to_string()
        .contains("requires a retained MetaStream record registry"));
}

#[test]
fn generated_f3d_rejects_act_binding_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_entities[0]
            .set_channel_guid(
                "Appearance",
                String::from("dddddddd-1111-2222-3333-eeeeeeeeeeee")
                    .try_into()
                    .unwrap(),
            )
            .unwrap();
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent ACT and appearance binding must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_act_record_index_edit_without_metastream_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ACT decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.act_component_links[0].record_index += 1;
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("an ACT record-index edit without its MetaStream index must fail");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("ACT component-link edit changes fields")
    ));
}
