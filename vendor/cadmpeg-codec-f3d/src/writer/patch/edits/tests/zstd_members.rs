// SPDX-License-Identifier: Apache-2.0
//! A retained F3D whose members are Zstandard (method 93) still patches.
//!
//! `zip` is built without its `zstd` feature, so it can neither expand nor write method 93:
//! the patch loop expands such a member with `cadmpeg-core`'s decoder, copies it back raw when
//! no edit changed it, and writes it as Deflate when one did.

use std::io::{Cursor, Read};

use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use zip::CompressionMethod;

use crate::records::recipes::CreationTimestamp;
use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;

/// `archive` with every member re-stored as Zstandard.
fn every_member_as_zstandard(archive: &[u8]) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let entries = (0..source.len())
        .map(|index| {
            let mut entry = source.by_index(index).unwrap();
            assert!(!entry.is_dir(), "the generated archive has no directories");
            let name = entry.name().expect("entry name").into_owned();
            let mut plain = Vec::new();
            entry.read_to_end(&mut plain).unwrap();
            RawEntry::compressed(
                &name,
                zip_fixture::ZSTD,
                &zip_fixture::zstd_stored_frame(&plain),
                &plain,
            )
        })
        .collect::<Vec<_>>();
    zip_fixture::archive(&entries)
}

/// Each member's name, method, and bytes as stored (still compressed).
fn stored_members(archive: &[u8]) -> Vec<(String, CompressionMethod, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index_raw(index).unwrap();
            let mut stored = Vec::new();
            entry.read_to_end(&mut stored).unwrap();
            (
                entry.name().expect("entry name").into_owned(),
                entry.compression(),
                stored,
            )
        })
        .collect()
}

#[test]
fn a_retained_f3d_with_zstandard_members_patches() {
    let mut source = cadmpeg_ir::examples::unit_cube().unwrap();
    let body = source.model.bodies[0].id.clone();
    f3d_native_mut(&mut source).creation_timestamps = vec![CreationTimestamp {
        id: "f3d:generated:creation-timestamp#0".into(),
        target: AttributeTarget::Body(body),
        record_index: 0,
        unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1_579_392_000_000_001.0).unwrap(),
    }];
    let mut generated = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut generated))
        .expect("generate a timestamp carrier");
    let retained = every_member_as_zstandard(&generated);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(retained.clone()),
            &DecodeOptions::default(),
        )
        .expect("decode the all-Zstandard archive");
    let (mut target, _, fidelity) = decoded.into_parts();
    let expected = {
        let mut native = f3d_native_mut(&mut target);
        let timestamp = &mut native.creation_timestamps[0];
        timestamp.unix_microseconds =
            cadmpeg_ir::scalar::FiniteReal::new(timestamp.unix_microseconds.get() + 100.0).unwrap();
        native.creation_timestamps.clone()
    };

    let mut patched = Vec::new();
    let report = F3dCodec
        .plan(
            EncodeInput::new(&target, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut patched))
        .expect("patch an archive whose members are all Zstandard");
    assert!(matches!(
        report.write_path(),
        cadmpeg_ir::report::export::WritePath::Patched { .. }
    ));

    // A member still at method 93 was copied as stored, so its bytes are the source's; one
    // that an edit changed is Deflate, and the report says so by name.
    let before = stored_members(&retained);
    let after = stored_members(&patched);
    assert_eq!(
        after.iter().map(|(name, ..)| name).collect::<Vec<_>>(),
        before.iter().map(|(name, ..)| name).collect::<Vec<_>>()
    );
    let mut copied = 0;
    let mut rewritten = Vec::new();
    for ((name, method, stored), (_, _, source_stored)) in after.iter().zip(&before) {
        match *method {
            CompressionMethod::ZSTD => {
                assert_eq!(stored, source_stored, "{name} was not copied as stored");
                copied += 1;
            }
            CompressionMethod::Deflated => rewritten.push(name.clone()),
            other => panic!("{name} was written as {other:?}"),
        }
    }
    assert!(copied > 0, "no unedited Zstandard member was copied");
    assert!(!rewritten.is_empty(), "the edit changed no member");
    for name in &rewritten {
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains(name.as_str()) && note.contains("Deflate")),
            "no note names {name}: {:?}",
            report.notes
        );
    }

    let result = F3dCodec
        .decode(&mut Cursor::new(patched), &DecodeOptions::default())
        .expect("decode the patched archive");
    assert_eq!(f3d_native(result.ir()).creation_timestamps, expected);
}
