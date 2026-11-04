// SPDX-License-Identifier: Apache-2.0
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

use cadmpeg_core::container::ContainerRole;

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossKind;
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{
    f3d_with_text_brep, f3d_with_text_brep_stream, f3d_without_brep,
    synthetic_ambiguous_multi_brep_f3d,
};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

/// A document with no ASM BREP stream has no selected stream, so the geometry
/// and topology losses must not name a decode failure of one. Stating a cause
/// that was never reached misreports which carrier is missing.
#[test]
fn brep_less_part_reports_an_absent_stream_not_a_failed_decode() {
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("declares no ASM BREP stream"),
        "geometry loss must state the absent stream: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected, so none can have failed to decode: {geometry}"
    );

    let topology = message(F3dLossCode::TopologyNotTransferred.kind());
    assert!(
        topology.contains("declares no ASM BREP stream"),
        "topology loss must state the absent stream: {topology}"
    );

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
        "no ASM BREP stream (.smb/.smbh) was found in the container"
    );

    // The decode ran; advising the reader to run it is container-only advice.
    assert!(
        !decoded
            .report()
            .notes
            .iter()
            .any(|note| note.starts_with("container-level inspection only")),
        "full decode must not carry container-only advice: {:?}",
        decoded.report().notes
    );
}

/// A document whose only geometry carrier uses the text encoding does declare a
/// carrier. Reporting an absent stream names the wrong gap: the geometry is in
/// the archive and its encoding is not read. The two statements send a reader to
/// A geometry-less text carrier is reported as a carrier whose decode
/// produced nothing, not as an absent stream.
#[test]
fn a_text_only_carrier_without_geometry_is_reported_as_empty_not_absent() {
    let archive = f3d_with_text_brep(&[
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat",
        "FusionAssetName[Active]/Breps.BlobParts/BREP1.sat",
    ]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("text-encoded ASM stream(s)") && geometry.contains("BREP0.sat"),
        "geometry loss must name the empty text carrier: {geometry}"
    );
    assert!(
        !geometry.contains("declares no ASM BREP stream"),
        "a text carrier is a declared carrier: {geometry}"
    );

    let topology = message(F3dLossCode::TopologyNotTransferred.kind());
    assert!(
        topology.contains("text-encoded"),
        "topology loss must name the encoding: {topology}"
    );

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
        "2 ASM BREP stream(s) are present in the text encoding (.sat/.smt) and produced no \
         geometry; no binary stream (.smb/.smbh) was found"
    );
}

/// A text carrier with a complete solid decodes through the shared B-rep
/// path: the loopless closed sphere face reaches the model arenas with its
/// radius in millimetres per the stream's unit rule.
#[test]
fn a_text_carrier_with_geometry_decodes_through_the_shared_brep_path() {
    let mut text = String::new();
    text.push_str("23200 0 2 2 \n");
    text.push_str("16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n");
    text.push_str("1 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @13 232.4.0.65535 #\n");
    text.push_str("body $-1 -1 $-1 $2 $-1 $-1 #\n");
    text.push_str("lump $-1 -1 $-1 $-1 $3 $1 #\n");
    text.push_str("shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n");
    text.push_str("face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n");
    text.push_str("sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n");
    text.push_str("End-of-ASM-data\n");

    let base = f3d_without_brep("part-design", "part.f3d", &[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let name = entry.name().expect("entry name").into_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/BREP0.sat", stored)
        .unwrap();
    zip.write_all(text.as_bytes()).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let kernel = decoded
        .report()
        .dialects()
        .as_ref()
        .expect("the report is classified")
        .iter()
        .find(|matched| matched.format() == "acis")
        .expect("text carrier has a shared kernel identity");
    assert_eq!(kernel.dialect().as_str(), "acis:text-asm");
    assert_eq!(
        kernel.declared()["carrier"],
        "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat"
    );
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    let surface = &decoded.ir().model.surfaces[0];
    let Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) = surface.geometry.solved() else {
        panic!("sphere carrier expected, got {:?}", surface.geometry);
    };
    let radius = sphere_surface.radius().get();
    // 25 stream units at scale 1 (millimetres per unit) are 25 mm.
    assert!((radius - 25.0).abs() < 1.0e-9);
}

/// Several BREP streams with no Design body map leave the selection ambiguous.
/// The streams are present, so a note claiming none was found is false; the
/// finding is that none of them is identified as the document's geometry.
#[test]
fn ambiguous_brep_selection_reports_the_streams_that_are_present() {
    let archive = synthetic_ambiguous_multi_brep_f3d();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(archive),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let message = |code: LossKind| {
        let loss = decoded
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code:?} not reported: {:?}", decoded.report().losses));
        loss.message.clone()
    };

    assert_eq!(
        message(F3dLossCode::MissingGeometryStream.kind()),
        "2 ASM BREP stream(s) are present, but none of them was selected as the document's \
         geometry stream"
    );
    let geometry = message(F3dLossCode::GeometryNotTransferred.kind());
    assert!(
        geometry.contains("2 BREP stream(s) were located") && geometry.contains("ambiguous"),
        "geometry loss must state the ambiguous selection: {geometry}"
    );
    assert!(
        !geometry.contains("selected stream"),
        "no stream was selected: {geometry}"
    );
}

#[test]
fn corrupt_kernel_carrier_is_reported_beside_valid_kernel_layer() {
    let base = f3d_without_brep("part-design", "part.f3d", &[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let name = entry.name().expect("entry name").into_owned();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    let valid_path = "FusionAssetName[Active]/Breps.BlobParts/BREP.valid.smb";
    zip.start_file(valid_path, stored).unwrap();
    zip.write_all(&synthetic_smbh()).unwrap();
    let corrupt_path = "FusionAssetName[Active]/Breps.BlobParts/BREP.corrupt.smb";
    zip.start_file(corrupt_path, stored).unwrap();
    zip.write_all(b"not an ACIS kernel stream").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let inspected = F3dCodec
        .inspect(
            &mut Cursor::new(archive.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(inspected.losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::KernelCarrierUnparseable.kind()
            && loss.message.contains(corrupt_path)
    }));

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(archive),
            &DecodeOptions {
                depth: cadmpeg_core::decode::DecodeDepth::Container,
                ..DecodeOptions::default()
            },
        )
        .unwrap();

    let kernel_layers = decoded
        .report()
        .dialects()
        .as_ref()
        .expect("the report is classified")
        .iter()
        .filter(|matched| matched.format() == "acis")
        .collect::<Vec<_>>();
    assert_eq!(kernel_layers.len(), 2);
    assert_eq!(kernel_layers[0].declared()["carrier"], valid_path);
    assert_eq!(kernel_layers[0].instance(), Some(valid_path));
    assert_eq!(kernel_layers[1].dialect().as_str(), "acis:unknown");
    assert_eq!(kernel_layers[1].declared()["carrier"], corrupt_path);
    assert_eq!(kernel_layers[1].instance(), Some(corrupt_path));
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::KernelCarrierUnparseable.kind())
        .expect("the corrupt carrier is visible as a loss");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(loss.message.contains(corrupt_path), "{}", loss.message);
}

/// The text encoding of an ASM stream carries the same entity model as the
/// binary one, so its archive members are geometry carriers and must classify as
/// such. Leaving them unclassified is what let the report state that a document
/// holding one declares no carrier at all.
#[test]
fn text_encoded_asm_members_classify_as_geometry_carriers() {
    for name in [
        "Fusion[Active]/Breps.BlobParts/BREP0.sat",
        "Fusion[Active]/Breps.BlobParts/BREP1.SAT",
        "probe/body.smt",
    ] {
        assert_eq!(
            crate::container::classify(name),
            ContainerRole::BrepText,
            "{name} must classify as a text-encoded BREP carrier"
        );
    }
    assert_eq!(
        crate::container::classify("a/b.smb"),
        ContainerRole::BrepSmb
    );
    assert_eq!(
        crate::container::classify("a/b.smbh"),
        ContainerRole::BrepSmbh
    );
}

#[test]
fn docstruct_metadata_distinguishes_absent_and_empty_subtype() {
    for (properties, expected_subtype) in [
        (
            r#"{"docstruct":{"version":"1.0.0","type":"part-design","attributes":{}}}"#,
            None,
        ),
        (
            r#"{"docstruct":{"version":"1.0.0","type":"part-design","subtype":"","attributes":{}}}"#,
            Some(""),
        ),
        (
            r#"{"docstruct":{"version":"1.0.0","type":"part-design","subtype":"part-standard","attributes":{}}}"#,
            Some("part-standard"),
        ),
    ] {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file("Properties.dat", stored).unwrap();
        zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
            .unwrap();
        zip.write_all(properties.as_bytes()).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        for depth in [
            cadmpeg_core::decode::DecodeDepth::Full,
            cadmpeg_core::decode::DecodeDepth::Container,
        ] {
            let decoded = F3dCodec
                .decode(
                    &mut Cursor::new(&archive),
                    &DecodeOptions {
                        depth,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let attributes = &decoded.ir().source.as_ref().unwrap().attributes;
            assert_eq!(
                attributes.get("docstruct_type").map(String::as_str),
                Some("part-design")
            );
            assert_eq!(
                attributes.get("docstruct_subtype").map(String::as_str),
                expected_subtype
            );
        }
    }
}

#[test]
fn decoded_text_brep_facts_keep_text_dialects_and_exclude_binary_routes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let prefix = concat!(
        "21800 0 2 3\n",
        "16 Autodesk Neutron 23 ASM 218.0.1.400 Unknown 9 Synthetic\n",
        "1 0.000001 0.0000000001\n",
        "asmheader $-1 -1 @11 218.0.1.400 #\n",
        "body $-1 -1 $-1 $2 $-1 $-1 #\n",
        "lump $-1 -1 $-1 $-1 $3 $1 #\n",
        "shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n",
        "face $-1 -1 $-1 $-1 $-1 $3 $-1 $5 forward single #\n",
        "sphere-surface $-1 -1 $-1 0 0 0 25 1 0 0 0 0 1 forward_v I I I I #\n",
    );
    for (terminator, expected) in [
        ("End-of-ASM-data", "acis:text-asm"),
        ("End-of-ACIS-data", "acis:text-acis"),
    ] {
        let text = format!("{prefix}{terminator}\n");
        let bytes = f3d_with_text_brep_stream(
            &["FusionAssetName[Active]/Breps.BlobParts/BREP0.sat"],
            text.as_bytes(),
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        assert_eq!(scan.text_breps.len(), 1);
        let expected_terminator = if terminator == "End-of-ASM-data" {
            cadmpeg_asm::sat::Terminator::Asm
        } else {
            cadmpeg_asm::sat::Terminator::Acis
        };
        assert!(matches!(
            scan.text_breps.get("FusionAssetName[Active]/Breps.BlobParts/BREP0.sat"),
            Some(crate::container::TextBrepFraming::Parsed(stream))
                if stream.terminator == expected_terminator
        ));
        let (facts, brep) = crate::decode::try_decode_text_model(&ctx, &scan)
            .unwrap()
            .expect("text sphere supplies model geometry");
        assert_eq!(brep.asm.faces.len(), 1);
        let framing = facts.kernel.as_ref().unwrap();
        let crate::container::KernelFraming::Text { header, .. } = framing else {
            panic!("text model facts must not fabricate binary framing")
        };
        assert!(header.has_history_partition());
        let matched = cadmpeg_asm::dialect::classify(framing.as_header_ref());
        assert_eq!(matched.dialect().as_str(), expected);
        assert!(!matched
            .declared()
            .contains_key(cadmpeg_asm::dialect::DECLARED_REFERENCE_WIDTH));
        assert!(crate::decode::try_decode_brep(&ctx, &scan, &facts)
            .unwrap()
            .is_none());
        scan.breps.push(facts);
        assert_eq!(crate::container::history_breps(&scan).count(), 0);
    }
}

#[test]
fn malformed_text_brep_returns_an_entry_named_error() {
    let entry = "FusionAssetName[Active]/Breps.BlobParts/BREP0.sat";
    let archive = f3d_with_text_brep_stream(&[entry], b"invalid SAT stream");
    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
    assert!(error
        .to_string()
        .contains(&format!("text BREP entry {entry} failed to parse:")));
}

#[test]
fn empty_model_maps_do_not_decode_or_blame_a_sketch_snapshot() {
    let snapshot = crate::test_support::smbh_geometry_test::synthetic_wire_body_smbh();
    let archive = crate::test_support::body_map_test::f3d_with_empty_model_maps(&snapshot, &[]);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let selection = crate::design::decode::body::design_model_selection(scan).unwrap();
        assert!(selection.explicitly_empty);
        assert!(selection.blob_names.is_empty());
    });
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.ir().model.curves.is_empty());
    let geometry = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::GeometryNotTransferred.kind())
        .unwrap();
    assert!(geometry
        .message
        .contains("No physical model BREP was selected"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("not a decodable BinaryFile")
            && loss.code != F3dLossCode::MissingGeometryStream.kind()));
}

#[test]
fn model_brep_reports_distinguish_failed_and_successful_empty_decodes() {
    use crate::test_support::smbh_header_test::smbh_header_prefix;
    use crate::test_support::tokens_test::{push_u8_string, t_end, t_ident};

    let mut empty = smbh_header_prefix();
    t_ident(&mut empty, "asmheader");
    push_u8_string(&mut empty, "231.6.3.65535");
    t_end(&mut empty);
    t_ident(&mut empty, "delta_state");
    empty.extend_from_slice(&[0; 16]);
    for (stream, expected) in [
        (b"invalid header".to_vec(), "not a decodable BinaryFile"),
        (empty, "decoded successfully"),
    ] {
        let archive = crate::test_support::zip_test::f3d_with_smbh(&stream);
        let decoded = F3dCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .unwrap();
        let losses = &decoded.report().losses;
        let geometry = losses
            .iter()
            .find(|loss| loss.code == F3dLossCode::GeometryNotTransferred.kind())
            .unwrap();
        assert!(geometry.message.contains(expected), "{}", geometry.message);
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::TopologyNotTransferred.kind()));
        assert!(losses
            .iter()
            .all(|loss| loss.code != F3dLossCode::MissingGeometryStream.kind()));
    }
}
