// SPDX-License-Identifier: Apache-2.0
//! Synthetic Fusion manifest ZIP entries.
#![allow(clippy::unwrap_used)]

use std::io::{Seek, Write};

pub(crate) fn write_synthetic_manifests<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
) {
    write_synthetic_manifests_with_version(
        zip,
        options,
        crate::manifest::TOP_LEVEL_MANIFEST_VERSION,
    );
}

/// The synthetic manifest pair, with the top-level manifest declaring
/// `version`. Every field after the version field is unchanged.
pub(super) fn write_synthetic_manifests_with_version<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
    version: &str,
) {
    zip.start_file("Manifest.dat", options).unwrap();
    zip.write_all(&crate::manifest::generated_top_level_with_version(version))
        .unwrap();
    zip.start_file(
        format!(
            "{}/Manifest.dat",
            crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
        ),
        options,
    )
    .unwrap();
    zip.write_all(&crate::manifest::generated_design_asset().unwrap())
        .unwrap();
}
