// SPDX-License-Identifier: Apache-2.0
//! Decode entry point with archive-session equivalence checks.

use std::io::{Cursor, SeekFrom};

use cadmpeg_core::decode::{DecodeArena, DecodeContext};
use cadmpeg_core::ReadSeek;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};

pub(crate) fn assert_equal(
    left: &Result<DecodeResult, DecodeFailure>,
    right: &Result<DecodeResult, DecodeFailure>,
) {
    match (left, right) {
        (Ok(left), Ok(right)) => {
            assert_eq!(
                serde_json::to_vec(left.ir()).unwrap(),
                serde_json::to_vec(right.ir()).unwrap()
            );
            assert_eq!(
                serde_json::to_vec(left.report()).unwrap(),
                serde_json::to_vec(right.report()).unwrap()
            );
            assert!(left == right, "decode result or source fidelity differs");
        }
        (Err(left), Err(right)) => {
            assert_eq!(left.to_string(), right.to_string());
            match (left, right) {
                (
                    DecodeFailure::StrictRejected { rejection: left },
                    DecodeFailure::StrictRejected { rejection: right },
                ) => {
                    assert_eq!(
                        serde_json::to_vec(left.report()).unwrap(),
                        serde_json::to_vec(right.report()).unwrap()
                    );
                }
                (
                    DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(left)),
                    DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(right)),
                ) => assert_eq!(left, right),
                _ => {}
            }
        }
        _ => panic!("codec and archive session differ: {left:?}, {right:?}"),
    }
}

pub(crate) fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, DecodeFailure> {
    let result = crate::F3dCodec.decode(reader, options);
    reader.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    let arena = DecodeArena::new();
    let Ok((ctx, root)) = DecodeContext::read_root(
        &mut Cursor::new(&bytes),
        &arena,
        &options.policy,
        options.depth,
    ) else {
        return result;
    };
    let session = match crate::F3dCodec.open_archive(&ctx, root) {
        Ok(session) => session,
        Err(error) => {
            if !matches!(error, cadmpeg_core::CodecError::WrongFormat(_)) {
                assert_equal(&result, &Err(error.into()));
            }
            return result;
        }
    };
    for order in 0..3 {
        let mut ids: Vec<_> = session.members().iter().map(crate::F3zMember::id).collect();
        if order == 1 {
            ids.reverse();
        } else if order == 2 && ids.len() > 1 {
            ids.rotate_left(1);
            for index in (1..ids.len()).step_by(2) {
                ids.swap(index - 1, index);
            }
        }
        let members = ids
            .into_iter()
            .map(|id| session.decode_member(id).unwrap())
            .collect::<Vec<_>>();
        assert_equal(&result, &session.compose(members));
    }
    result
}

pub(crate) fn deflated_archive(root: &str, members: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = crate::zip_write::file_options(zip::CompressionMethod::Deflated);
    for (name, bytes) in members {
        archive.start_file(*name, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    archive.start_file("Manifest.json", options).unwrap();
    archive
        .write_all(format!(r#"{{"root":"{root}"}}"#).as_bytes())
        .unwrap();
    archive
        .start_file("DesignDescription.json", options)
        .unwrap();
    archive.write_all(b"{}").unwrap();
    archive.finish().unwrap().into_inner()
}
