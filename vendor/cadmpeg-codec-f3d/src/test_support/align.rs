// SPDX-License-Identifier: Apache-2.0

use crate::layout::{align_scope_extended_prefix as extended, align_scope_prefix as compact};
use crate::records::feature::align::DesignAlignForm;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::u64_from_index;

pub(crate) fn align_prefix_fixture(form: DesignAlignForm) -> (Vec<u8>, DesignParameterScope) {
    let length = match form {
        DesignAlignForm::Compact => compact::LEN,
        DesignAlignForm::Extended => extended::LEN,
    };
    let shift = length - compact::LEN;
    let mut bytes = vec![0; length];
    if form == DesignAlignForm::Extended {
        bytes[extended::EXTRA_CODE..extended::EXTRA_CODE + 4]
            .copy_from_slice(&extended::EXTRA_CODE_VALUE.to_le_bytes());
    }
    for offset in [compact::FIRST_MARKER, compact::SECOND_MARKER] {
        bytes[offset + shift] = 1;
    }
    for (offset, value) in [
        (compact::FIRST_REFERENCE, 806_u32),
        (compact::SECOND_REFERENCE, 802),
        (compact::SOURCE_CODE, 7),
    ] {
        bytes[offset + shift..offset + shift + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut identity = Vec::new();
    super::lp_utf16(&mut identity, "00000000-0000-0000-0000-000000000000");
    bytes[compact::IDENTITY_LENGTH + shift..compact::TAIL_ZERO + shift].copy_from_slice(&identity);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#800",
        DesignFeatureKind::Align,
        800,
    );
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = u64_from_index(length);
            draft.reference_members = ReferenceRun::unlocated(vec![802, 804, 806]);
            draft.layout_fixture_references();
            draft.frame_length = draft.kind_offset + 10 + 72;
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

pub(crate) fn align_archive(
    form: DesignAlignForm,
    refused: bool,
    constructor_refused: bool,
) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let (mut bytes, _) = align_prefix_fixture(form);
    let shift = bytes.len() - compact::LEN;
    let mut header = Vec::new();
    super::indexed_header(&mut header, *b"256", 800);
    bytes[..header.len()].copy_from_slice(&header);
    if refused {
        bytes[compact::FIRST_MARKER + shift] = 0;
    }
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    for reference in [802, 804, 806] {
        super::push_marked_reference(&mut bytes, reference);
    }
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    super::lp_utf16(&mut bytes, "Align");
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    bytes.extend_from_slice(&[1, 15]);
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&252_u32.to_le_bytes());
    bytes.extend_from_slice(&0.25_f64.to_le_bytes());
    bytes.extend_from_slice(&252_u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 14]);
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.extend_from_slice(&[1, 13]);
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);
    super::indexed_header(&mut bytes, *b"257", 800);
    let (_, scope) = align_prefix_fixture(form);
    let mut constructor = align_constructor_fixture(&scope);
    if constructor_refused {
        constructor[crate::layout::align_constructor::FIRST_MARKER] = 0;
    }
    bytes.extend_from_slice(&constructor);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    super::manifest_test::write_synthetic_manifests(&mut zip, options);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
        .unwrap();
    zip.write_all(&bytes).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
        .unwrap();
    zip.write_all(&super::streams_test::design_metastream(&[]))
        .unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn align_constructor_fixture(scope: &DesignParameterScope) -> Vec<u8> {
    use crate::design::decode::scopes::align::construction::{CODE_OFFSETS, WORD_OFFSETS};
    use crate::layout::align_constructor as layout;
    let mut bytes = vec![0; layout::LEN];
    let mut header = Vec::new();
    super::indexed_header(&mut header, *b"650", 806);
    bytes[..header.len()].copy_from_slice(&header);
    for at in [
        layout::FIRST_MARKER,
        layout::SECOND_MARKER,
        layout::THIRD_MARKER,
        layout::FOURTH_MARKER,
        layout::OWNER_MARKER,
        layout::FIRST_COMPANION_MARKER,
        layout::SECOND_COMPANION_MARKER,
        layout::REPEATED_OWNER_MARKER,
    ] {
        bytes[at] = layout::FIRST_MARKER_VALUE;
    }
    for (at, value) in WORD_OFFSETS.into_iter().zip([
        814_u32,
        816,
        818,
        820,
        scope.record_index,
        822,
        824,
        scope.record_index,
    ]) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in CODE_OFFSETS.into_iter().zip([7_u32, 9, 3, 11, 17]) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in (0_u32..16).map(|value| f64::from(value) * 0.25).enumerate() {
        let at = layout::MATRIX_SCALARS + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[layout::SCALAR..layout::SCALAR + 8].copy_from_slice(&0.625_f64.to_le_bytes());
    super::indexed_header(&mut bytes, *b"651", 806);
    bytes
}

pub(crate) fn align_construction_value_fixture(
    scope: &DesignParameterScope,
) -> crate::records::feature::align::DesignAlignConstruction {
    use crate::design::decode::scopes::align::construction::{CODE_OFFSETS, WORD_OFFSETS};
    use crate::layout::align_constructor as layout;
    use crate::records::feature::align::DesignAlignConstruction;
    use crate::records::identity::Located;
    use crate::records::references::DesignClassTag;
    use cadmpeg_ir::scalar::FiniteReal;
    let words = [
        814,
        816,
        818,
        820,
        scope.record_index,
        822,
        824,
        scope.record_index,
    ];
    let codes = [7, 9, 3, 11, 17];
    DesignAlignConstruction {
        record_index: 806,
        byte_offset: 0,
        paired_byte_offset: u64_from_index(layout::LEN),
        class_tag: DesignClassTag::try_from("650".to_owned()).unwrap(),
        paired_class_tag: DesignClassTag::try_from("651".to_owned()).unwrap(),
        marked_words: std::array::from_fn(|ordinal| Located {
            value: words[ordinal],
            offset: u64_from_index(WORD_OFFSETS[ordinal]),
        }),
        codes: std::array::from_fn(|ordinal| Located {
            value: codes[ordinal],
            offset: u64_from_index(CODE_OFFSETS[ordinal]),
        }),
        matrix_scalars: Located {
            value: std::array::from_fn(|ordinal| {
                FiniteReal::new(f64::from(u32::try_from(ordinal).unwrap()) * 0.25).unwrap()
            }),
            offset: u64_from_index(layout::MATRIX_SCALARS),
        },
        scalar: Located {
            value: FiniteReal::new(0.625).unwrap(),
            offset: u64_from_index(layout::SCALAR),
        },
    }
}
