// SPDX-License-Identifier: Apache-2.0
//! Synthetic assembly and `.f3z` ZIP builders.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Read, Write};

use zip::CompressionMethod;

use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::test_support::streams_test::design_metastream_with_records;
use crate::test_support::{cross_document_reference, local_reference};

/// A `RedirectionsStream.dat` body with one self design entry plus one design
/// and one XREF reference per `(relative_path, role)` pair.
pub(crate) fn redirections_json(own_name: &str, targets: &[(&str, &str)]) -> String {
    let mut designs = vec![format!(
        r#"{{"file-version":1,"targetFileName":"{own_name}","displayName":"root","lineageUrn":"urn:adsk.wipprod:dm.lineage:RootKey","versionUrn":"urn:adsk.wipprod:fs.file:vf.RootKey?version=1"}}"#
    )];
    let mut references = Vec::new();
    for (ordinal, (path, role)) in targets.iter().enumerate() {
        designs.push(format!(
            r#"{{"file-version":1,"targetFileName":"{path}","displayName":"component{ordinal}","lineageUrn":"urn:adsk.wipprod:dm.lineage:Key{ordinal}","versionUrn":"urn:adsk.wipprod:fs.file:vf.Key{ordinal}?version=1"}}"#
        ));
        references.push(format!(
            r#"{{"from":"{own_name}","relativePath":"{path}","type":"XREF","properties":[{{"neutronRole":{{"value":"{role}","dataType":"STRING"}}}},{{"neutronData":{{"value":"{role}","dataType":"STRING"}}}}]}}"#
        ));
    }
    let references = if references.is_empty() {
        "{}".to_owned()
    } else {
        format!("[{}]", references.join(","))
    };
    format!(
        r#"{{"name":"RedirectionsStream","schema-version":0,"designs":[{}],"references":{references}}}"#,
        designs.join(",")
    )
}

/// A BREP-less `.f3d` with a docstruct `Properties.dat` and a redirections
/// table referencing `targets`.
pub(crate) fn f3d_without_brep(
    doc_type: &str,
    own_name: &str,
    targets: &[(&str, &str)],
) -> Vec<u8> {
    let redirections = redirections_json(own_name, targets);
    f3d_with_redirections_json(doc_type, redirections.as_bytes())
}

/// A BREP-less `.f3d` with a complete XREF placement carrier for one target.
/// The Design `MetaStream` selects the modern tagged tail, so the placement
/// exercises the same binding route as a decoded document.
pub(crate) fn f3d_without_brep_with_xref_placement(
    doc_type: &str,
    own_name: &str,
    target: &str,
    role: &str,
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    f3d_with_xref_placements(doc_type, own_name, &[(target, role, transform)], None)
}

pub(crate) fn f3d_with_identity_xrefs(
    doc_type: &str,
    own_name: &str,
    targets: &[(&str, &str)],
) -> Vec<u8> {
    let mut roles = std::collections::BTreeSet::new();
    let placements = targets
        .iter()
        .filter(|(_, role)| roles.insert(*role))
        .map(|(target, role)| (*target, *role, crate::xref::IDENTITY_ROWS))
        .collect::<Vec<_>>();
    let redirections = redirections_json(own_name, targets);
    f3d_with_xref_placements(
        doc_type,
        own_name,
        &placements,
        Some(redirections.as_bytes()),
    )
}

pub(crate) fn f3d_with_reference_table_and_identity_xrefs(
    doc_type: &str,
    own_name: &str,
    targets: &[(&str, &str)],
    redirections: &[u8],
) -> Vec<u8> {
    let placements = targets
        .iter()
        .map(|(target, role)| (*target, *role, crate::xref::IDENTITY_ROWS))
        .collect::<Vec<_>>();
    f3d_with_xref_placements(doc_type, own_name, &placements, Some(redirections))
}

fn f3d_with_xref_placements(
    doc_type: &str,
    own_name: &str,
    placements: &[(&str, &str, [[f64; 4]; 4])],
    redirections: Option<&[u8]>,
) -> Vec<u8> {
    if placements.is_empty() {
        let generated = redirections_json(own_name, &[]);
        return f3d_with_redirections_json(doc_type, redirections.unwrap_or(generated.as_bytes()));
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    let properties = format!(
        r#"{{"docstruct":{{"version":"1.0.0","type":"{doc_type}","subtype":"synthetic","attributes":{{}}}}}}"#
    );
    zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
        .unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.start_file("ComponentReferenceData.json", stored)
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    let targets = placements
        .iter()
        .map(|(target, role, _)| (*target, *role))
        .collect::<Vec<_>>();
    let generated = redirections_json(own_name, &targets);
    zip.write_all(redirections.unwrap_or(generated.as_bytes()))
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    let mut bulk = Vec::new();
    let records = placements
        .iter()
        .enumerate()
        .map(|(ordinal, (_, role, transform))| {
            let index = 10 + u32::try_from(ordinal).unwrap();
            let offset = u64::try_from(bulk.len()).unwrap();
            bulk.extend(xref_placement_record(role, *transform, index));
            (u64::from(index), offset)
        })
        .collect::<Vec<_>>();
    let indices = records.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    zip.write_all(&design_metastream_with_records(
        &[(
            "CE2913AA-CFE0-4F04-9102-24424ED3BCFA",
            "",
            2,
            "Component",
            &indices,
        )],
        &records,
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bulk).unwrap();
    zip.finish().unwrap().into_inner()
}

/// Build the complete modern Design occurrence-placement record used by the
/// focused archive controls.
fn xref_placement_record(role: &str, transform: [[f64; 4]; 4], index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&u64::from(index).to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(cross_document_reference(100, role));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    let identity = transform == crate::xref::IDENTITY_ROWS;
    bytes.push(u8::from(identity));
    if !identity {
        for value in transform.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(7));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

/// Build an F3D with caller-supplied Redirections bytes for admission tests.
pub(crate) fn f3d_with_redirections_json(doc_type: &str, redirections: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    let properties = format!(
        r#"{{"docstruct":{{"version":"1.0.0","type":"{doc_type}","subtype":"synthetic","attributes":{{}}}}}}"#
    );
    zip.write_all(&u32::try_from(properties.len()).unwrap().to_le_bytes())
        .unwrap();
    zip.write_all(properties.as_bytes()).unwrap();
    zip.start_file("ComponentReferenceData.json", stored)
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections).unwrap();
    zip.finish().unwrap().into_inner()
}

/// A minimal ASM text stream: the three header lines, an `asmheader`, one `body`
/// record, and the terminator. Written from the encoding's own structure so the
/// fixture exercises classification without carrying a decodable payload.
fn synthetic_asm_text_stream() -> Vec<u8> {
    let mut text = String::new();
    text.push_str("21800 0 1 12           \n");
    text.push_str("16 Autodesk Neutron 23 ASM 218.0.1.400 Unknown 9 Synthetic \n");
    text.push_str("10 9.999999999999999547e-07 1.000000000000000036e-10 \n");
    text.push_str("asmheader $-1 -1 @11 218.0.1.400 #\n");
    text.push_str("body $-1 -1 $-1 $-1 $-1 $-1 #\n");
    text.push_str("End-of-ASM-data\n");
    text.into_bytes()
}

/// A BREP-less `.f3d` whose `Breps.BlobParts` holds text-encoded ASM members
/// only. This is the shape of an early-generation archive: the text streams
/// are the document's geometry carriers.
pub(crate) fn f3d_with_text_brep(members: &[&str]) -> Vec<u8> {
    f3d_with_text_brep_stream(members, &synthetic_asm_text_stream())
}

/// A BREP-less `.f3d` with the supplied text kernel stream under each member.
pub(crate) fn f3d_with_text_brep_stream(members: &[&str], stream: &[u8]) -> Vec<u8> {
    let base = f3d_without_brep("part-design", "part.f3d", &[]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut source = zip::ZipArchive::new(Cursor::new(base)).unwrap();
    for i in 0..source.len() {
        let mut entry = source.by_index(i).unwrap();
        let name = entry.name().expect("entry name").into_owned();
        let mut bytes = Vec::new();
        Read::read_to_end(&mut entry, &mut bytes).unwrap();
        zip.start_file(name, stored).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    for member in members {
        zip.start_file(*member, stored).unwrap();
        zip.write_all(stream).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Wrap members into a `.f3z` archive with `Manifest.json` naming the root.
pub(crate) fn f3z_archive(root_name: &str, members: &[(&str, &[u8])]) -> Vec<u8> {
    f3z_archive_with_design_description(
        root_name,
        members,
        br#"{"name":"Autodesk Design Description","version":"0.1","designDescription":{"id":"0","designGraphs":[]}}"#,
    )
}

pub(crate) fn f3z_archive_with_design_description(
    root_name: &str,
    members: &[(&str, &[u8])],
    design_description: &[u8],
) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, bytes) in members {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.start_file("Manifest.json", stored).unwrap();
    zip.write_all(format!(r#"{{"root":"{root_name}"}}"#).as_bytes())
        .unwrap();
    zip.start_file("DesignDescription.json", stored).unwrap();
    zip.write_all(design_description).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) const XREF_ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";

/// Two BREP streams, no history partition to break the tie and no `Design1`
/// pair to select the legacy complete set.
pub(crate) fn synthetic_ambiguous_multi_brep_f3d() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for name in ["first", "second"] {
        let mut smb = synthetic_smbh();
        smb[39..47].copy_from_slice(&2u64.to_le_bytes()); // no history partition
        smb.truncate(60);
        zip.start_file(
            format!("FusionAssetName[Active]/Breps.BlobParts/BREP.{name}.smb"),
            stored,
        )
        .unwrap();
        zip.write_all(&smb).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
