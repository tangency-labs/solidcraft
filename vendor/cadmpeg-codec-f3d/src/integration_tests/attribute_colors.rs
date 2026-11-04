// SPDX-License-Identifier: Apache-2.0
//! Attribute-chain color decoding over synthetic ASM records.

use crate::test_support::tokens_test::{push_u8_string, t_dbl, t_end, t_ident, t_ref, t_subident};

#[test]
fn rgb_attribute_chain_decodes_body_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "body");
    t_ref(&mut bytes, 1); // attrib-chain head
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1); // next attrib
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("fixture value fits i64"), r))
        .collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (0.1, 0.2, 0.3, 1.0)
    );
}

#[test]
fn truecolor_attribute_chain_decodes_by_color_as_opaque_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "truecolor");
    t_subident(&mut bytes, "adesk");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    bytes.push(0x17);
    bytes.extend_from_slice(&(0xc240_80c0i64).to_le_bytes());
    t_end(&mut bytes);

    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("fixture value fits i64"), r))
        .collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_attribute_chain_decodes_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    push_u8_string(&mut bytes, "4227264"); // 0x4080c0
    t_end(&mut bytes);

    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("fixture value fits i64"), r))
        .collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_rejects_non_decimal_and_overwide_values() {
    use std::collections::HashMap;

    for value in ["", "+4227264", "0x4080c0", "16777216"] {
        let mut bytes = Vec::new();
        t_ident(&mut bytes, "face");
        t_ref(&mut bytes, 1);
        t_end(&mut bytes);
        t_subident(&mut bytes, "entatt_color");
        t_subident(&mut bytes, "bt");
        t_ident(&mut bytes, "attrib");
        t_ref(&mut bytes, -1);
        push_u8_string(&mut bytes, value);
        t_end(&mut bytes);

        let records = cadmpeg_asm::test_support::sab::frame(
            &bytes,
            0,
            bytes.len(),
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .unwrap();
        let by_index: HashMap<i64, _> = records
            .iter()
            .map(|r| (i64::try_from(r.index).expect("fixture value fits i64"), r))
            .collect();
        assert!(
            cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).is_none()
        );
    }
}

#[test]
fn invalid_color_attribute_does_not_hide_later_chain_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, 2);
    push_u8_string(&mut bytes, "not-a-color");
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("fixture value fits i64"), r))
        .collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (0.1, 0.2, 0.3, 1.0)
    );
}
