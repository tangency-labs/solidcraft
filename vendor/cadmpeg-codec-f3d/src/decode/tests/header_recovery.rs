// SPDX-License-Identifier: Apache-2.0
//! Independently framed SMT headers do not control B-rep survival.

use crate::test_support::assembly_test::f3d_with_text_brep_stream;
use crate::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

const SPHERE_SMT: &str = concat!(
    "23200 0 2 2\n",
    "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic\n",
    "1 0.000001 0.0000000001\n",
    "asmheader $-1 -1 @13 232.4.0.65535 #\n",
    "body $-1 -1 $-1 $2 $-1 $-1 #\n",
    "lump $-1 -1 $-1 $-1 $3 $1 #\n",
    "shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n",
    "face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n",
    "sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n",
    "End-of-ASM-data\n",
);

#[test]
fn malformed_smt_metadata_keeps_geometry_and_reports_recovery() {
    let original = SPHERE_SMT;
    let entry = "FusionAssetName[Active]/Breps.BlobParts/Body1.smt";
    let decode = |text: &str| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_text_brep_stream(&[entry], text.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(original);
    assert_eq!(expected.ir().model.faces.len(), 1);
    for source in [
        original.replace("9 Synthetic", ""),
        original.replace(
            "16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic",
            "",
        ),
        original.replace("0.000001", "invalid"),
        original.replace("0.0000000001", "NaN"),
        original.replace("0.0000000001", ""),
        original.replacen(" 0 2 2", " -1 2 2", 1),
        original.replacen(" 0 2 2", " 4294967296 2 2", 1),
        original.replacen(" 0 2 2", " invalid 2 2", 1),
    ] {
        let recovered = decode(&source);
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered.report().losses.iter().any(|loss| loss.code
            == crate::loss::F3dLossCode::TextHeaderNoncanonical
                .note("")
                .code));
        if source.contains("invalid") {
            assert_eq!(
                recovered.ir().tolerances.angular,
                expected.ir().tolerances.angular
            );
        } else if source.contains("NaN") {
            assert_eq!(
                recovered.ir().tolerances.linear,
                expected.ir().tolerances.linear
            );
        }
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::report::Severity::Error));
    }
}

#[test]
fn binary_product_encoding_recovery_keeps_independent_geometry() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh;
    let original = synthetic_geometry_smbh();
    let decode = |source: &[u8]| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(source)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    assert!(!expected.ir().model.faces.is_empty());
    let mut cursor = 47; // ASM BinaryFile8 fixed words; docs/layouts/asm.toml.
    for _ in 0..3 {
        let mut source = original.clone();
        source[cursor + 2] = 0xff;
        let recovered = decode(&source);
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert_eq!(recovered.ir().tolerances, expected.ir().tolerances);
        assert!(recovered.report().losses.iter().any(
            |loss| loss.code == crate::loss::F3dLossCode::KernelHeaderMetadataUnresolved.kind()
        ));
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .is_ok());
        cursor += 2 + usize::from(original[cursor + 1]);
    }
}

#[test]
fn smt_tolerance_overflow_in_document_units_keeps_geometry() {
    let original = SPHERE_SMT.replace("1 0.000001", "10 0.000001");
    let modified = original.replace("0.000001", "1.7976931348623157e308");
    let entry = "FusionAssetName[Active]/Breps.BlobParts/Body1.smt";
    let decode = |source: &str| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_text_brep_stream(&[entry], source.as_bytes())),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    let recovered = decode(&modified);
    assert_eq!(recovered.ir().model, expected.ir().model);
    assert_eq!(
        recovered.ir().tolerances.linear,
        cadmpeg_ir::units::Tolerances::default().linear
    );
    assert_eq!(
        recovered.ir().tolerances.angular,
        expected.ir().tolerances.angular
    );
    assert!(recovered
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved.kind()));
}

#[test]
fn invalid_binary_kernel_tolerances_keep_independent_geometry() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::smbh_header_test::smbh_header_prefix;
    use crate::test_support::zip_test::f3d_with_smbh;
    let original = synthetic_geometry_smbh();
    let decode = |source: &[u8]| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(source)),
                &DecodeOptions::default(),
            )
            .unwrap()
    };
    let expected = decode(&original);
    assert!(!expected.ir().model.faces.is_empty());
    // The header builder ends with three tagged doubles; each payload is 8 bytes.
    let header_end = smbh_header_prefix().len();
    for (offset, values) in [
        (
            header_end - 17,
            &[f64::NAN, f64::INFINITY, -1.0, 0.0, f64::MAX][..],
        ),
        (header_end - 8, &[f64::NAN, f64::INFINITY, -1.0, 0.0][..]),
    ] {
        for value in values {
            let mut modified = original.clone();
            modified[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            let recovered = decode(&modified);
            assert_eq!(recovered.ir().model, expected.ir().model);
            assert!(recovered.report().losses.iter().any(|loss| loss.code
                == crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved.kind()));
            assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
                .unwrap()
                .is_ok());
        }
    }
}

#[test]
fn manifest_metadata_recovery_preserves_geometry_and_exact_archive() {
    use std::io::{Read, Write};
    let original = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let expected = F3dCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    for field in 0..5 {
        let mut source = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if entry.name().expect("entry name") == "Manifest.dat" {
                if field == 0 {
                    data[4] = 0xff;
                } else {
                    let mut cursor = 0;
                    for _ in 0..2 {
                        let count = usize::try_from(u32::from_le_bytes(
                            data[cursor..cursor + 4].try_into().unwrap(),
                        ))
                        .expect("fixture string length fits usize");
                        cursor += 4 + count;
                    }
                    for _ in 0..field {
                        let count = usize::try_from(u32::from_le_bytes(
                            data[cursor..cursor + 4].try_into().unwrap(),
                        ))
                        .expect("fixture string length fits usize");
                        cursor += 4 + 2 * count;
                    }
                    data[cursor + 4..cursor + 6].copy_from_slice(&0xd800u16.to_le_bytes());
                }
            }
            output
                .start_file(
                    entry.name().expect("entry name"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            output.write_all(&data).unwrap();
        }
        let bytes = output.finish().unwrap().into_inner();
        let recovered = cadmpeg_test_support::EditableDecodeResult::from(
            F3dCodec
                .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
                .unwrap(),
        );
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::ManifestMetadataUnresolved.kind()));
        assert!(cadmpeg_ir::validate_neutral(recovered.ir(), Vec::new())
            .unwrap()
            .is_ok());
        let mut replay = Vec::new();
        crate::test_support::plan_inherited_write(
            recovered.ir(),
            recovered.source_fidelity(),
            &mut replay,
        )
        .unwrap();
        assert_eq!(replay, bytes);
    }
}
