// SPDX-License-Identifier: Apache-2.0
//! Code-built saved scene streams and archives.

use std::io::{Cursor, Write};

fn text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend(
        u32::try_from(value.encode_utf16().count())
            .unwrap()
            .to_le_bytes(),
    );
    for unit in value.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
}

pub(crate) fn override_row(path: &[u64]) -> Vec<u8> {
    override_row_tagged(path, path.len())
}

pub(crate) fn override_row_tagged(path: &[u64], tagged_from: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    text(&mut bytes, "OverrideTransformAttribute");
    bytes.extend([0, 0, 0, 0, 1]);
    for value in [
        1_f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 2., 3., 4., 1.,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([1, 1]);
    text(&mut bytes, "PersistentPassiveNodePath");
    text(&mut bytes, "0x123abc");
    bytes.extend(u32::try_from(path.len()).unwrap().to_le_bytes());
    for (position, index) in path.iter().enumerate() {
        bytes.extend(if position < tagged_from {
            [0; 4]
        } else {
            [1, 0, 0, 0]
        });
        bytes.extend(index.to_be_bytes());
    }
    bytes
}

pub(crate) fn archive(world: &[u8]) -> Vec<u8> {
    let bytes =
        super::zip_test::f3d_with_smbh(&super::smbh_geometry_test::synthetic_geometry_smbh());
    with_world(bytes, world)
}

pub(crate) fn with_world(bytes: Vec<u8>, world: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(bytes)).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/OGS.BlobFolder/OGS/DefaultScene/world",
        crate::zip_write::file_options(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(world).unwrap();
    zip.finish().unwrap().into_inner()
}
