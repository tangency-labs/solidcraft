// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion `.protein` appearance assets and bind them to B-rep bodies.
//!
//! Material and appearance semantics are defined in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
//! [`decode`] reads appearance records without resolving body bindings.
//! [`decode_with_body_bindings`] joins Protein assets, Design assignments, ACT
//! channels, and blob-qualified Design body-map bindings through the
//! design-entity join backbone in
//! [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use crate::records::references::DesignVisualToken;
use cadmpeg_core::container::ContainerRole;

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use crate::records::{bodies::DesignBodyBinding, references::DesignMaterialAssignment};
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Color;
use cadmpeg_protein::appearance::{
    is_physical_schema, neutral_property_name, texture_asset, TextureAssetResult,
};
use cadmpeg_protein::{
    CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
};

use crate::bytes::{is_guid_prefix, lp_utf16_bounded, skip_lp_u32_bytes, take_lp_utf8};
use crate::container::ContainerScan;
use crate::design::presentation::{
    APPEARANCE_LIBRARY_ID, GUID_LEN, MODERN_APPEARANCE_LIBRARY_IDS as APPEARANCE_LIBRARY_ID_PAIR,
};
use crate::writer::primitives::zero_fill_to;
/// The `AssetLibID` [`encode_protein`] writes for an appearance that names no
/// library. A stored library identifier is a library GUID or a library path;
/// the null GUID names neither.
const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

/// The library identifier an `InstanceProperties` record stores, when it names
/// a library.
fn library_id(asset_lib_id: &str) -> Option<String> {
    if asset_lib_id.is_empty() || asset_lib_id == NO_ASSET_LIB_ID {
        return None;
    }
    Some(asset_lib_id.to_owned())
}

fn named_act_channels(
    owner: std::fmt::Arguments<'_>,
    channels: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    Ok(cadmpeg_core::text::named_entries(
        owner,
        channels.cloned().unwrap_or_default(),
    )?)
}

fn lp_ascii_printable(bytes: &[u8], at: usize) -> Result<Option<(String, usize)>, CodecError> {
    let Some(length) = View::u32_le_at(bytes, at).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if !(1..=64).contains(&length) {
        return Ok(None);
    }
    let Some(start) = at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = start.checked_add(length) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(start..end) else {
        return Ok(None);
    };
    if !raw.iter().all(|byte| (0x20..0x7f).contains(byte)) {
        return Ok(None);
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| CodecError::Malformed("F3D printable ASCII validation failed".into()))?;
    Ok(Some((text.to_owned(), end)))
}

pub(crate) fn encode_protein(appearance: &Appearance) -> Result<Vec<u8>, CodecError> {
    if !appearance.textures.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D cannot synthesize connected Protein texture assets".into(),
        ));
    }
    let schema = appearance.schema.as_deref().unwrap_or("GenericSchema");
    let guid = appearance
        .visual_guid
        .as_deref()
        .or(appearance.asset_guid.as_deref())
        .ok_or_else(|| {
            CodecError::Malformed("source-less appearance lacks an asset GUID".into())
        })?;
    let name = appearance.name.as_deref().unwrap_or("Prism-001");
    let mut logical = RECORD_MARKER.to_vec();
    for value in [
        schema,
        guid,
        name,
        appearance.library_id.as_deref().unwrap_or(NO_ASSET_LIB_ID),
    ] {
        push_lp(&mut logical, value)?;
    }
    let value_block = logical.len();
    match schema {
        "GenericSchema" => {
            zero_fill_to(&mut logical, value_block + 209);
            write_color(&mut logical, value_block + 112, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("reflectivity_at_0deg") {
                logical[value_block + 171..value_block + 175].copy_from_slice(b"\x0c\x00\x00\x00");
                logical[value_block + 175..value_block + 183]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
                logical[value_block + 201..value_block + 209]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PrismOpaqueSchema" | "PrismMetalSchema" => {
            zero_fill_to(&mut logical, value_block + 96);
            write_color(&mut logical, value_block + 8, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("surface_roughness") {
                logical[value_block + 64..value_block + 68].copy_from_slice(b"\x0e\x20\x00\x00");
                logical[value_block + 68..value_block + 76]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PrismTransparentSchema" => {
            zero_fill_to(&mut logical, value_block + 177);
            write_color(&mut logical, value_block + 121, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 169..value_block + 177]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => zero_fill_to(&mut logical, value_block + 8),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "source-less Protein schema {schema} is unsupported"
            )));
        }
    }
    let instance = page_logical(&logical)?;
    let mut catalog = RECORD_MARKER.to_vec();
    push_lp(&mut catalog, schema)?;
    catalog.push(0);
    push_lp(&mut catalog, name)?;
    push_lp(&mut catalog, name)?;
    catalog.extend_from_slice(&2_u32.to_le_bytes());
    push_lp(
        &mut catalog,
        appearance.category.as_deref().unwrap_or("Generated"),
    )?;
    push_lp(&mut catalog, "Default")?;
    push_lp(&mut catalog, "")?;
    catalog.extend_from_slice(&0_u32.to_le_bytes());
    catalog.extend_from_slice(&1_u32.to_le_bytes());
    push_lp(&mut catalog, "")?;
    let catalog = page_logical(&catalog)?;
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("AssetData/InstanceProperties.bin", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create Protein instance: {error}"))
        })?;
    zip.write_all(&instance)?;
    zip.start_file("AssetData/DefinitionIteratorProperties.bin", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create Protein catalog: {error}"))
        })?;
    zip.write_all(&catalog)?;
    Ok(zip
        .finish()
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot finish Protein asset: {error}"))
        })?
        .into_inner())
}

fn push_lp(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = u32::try_from(value.len())
        .map_err(|_| CodecError::Malformed("Protein string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_color(out: &mut [u8], offset: usize, color: Option<Color>) -> Result<(), CodecError> {
    let color = color.ok_or_else(|| {
        CodecError::Malformed("visual source-less Protein appearance lacks base_color".into())
    })?;
    for (ordinal, value) in [color.r(), color.g(), color.b(), color.a()]
        .into_iter()
        .enumerate()
    {
        let at = offset + ordinal * 8;
        out[at..at + 8].copy_from_slice(&f64::from(value).to_le_bytes());
    }
    Ok(())
}

fn page_logical(logical: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        &u32::try_from(PAGE_SIZE)
            .map_err(|_| CodecError::malformed("Protein page size exceeds u32"))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let first = logical.len().min(PAGE_SIZE - 4);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    zero_fill_to(&mut bytes, STREAM_HEADER_LEN + PAGE_SIZE);
    let mut rest = &logical[first..];
    while rest.len() > PAGE_SIZE - 8 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(CONTINUATION_MARKER);
        bytes.extend_from_slice(&rest[..PAGE_SIZE - 8]);
        rest = &rest[PAGE_SIZE - 8..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(TERMINAL_MARKER);
        let length = u16::try_from(rest.len())
            .map_err(|_| CodecError::Malformed("Protein tail page exceeds u16::MAX".into()))?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let end = STREAM_HEADER_LEN + (bytes.len() - STREAM_HEADER_LEN).next_multiple_of(PAGE_SIZE);
        zero_fill_to(&mut bytes, end);
    }
    Ok(bytes)
}

#[derive(Default)]
pub(crate) struct ProteinAppearanceEdit {
    pub(crate) color: Option<Color>,
    pub(crate) properties: BTreeMap<String, f64>,
}

pub(crate) fn patch_protein_appearances(
    protein: &[u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
    notes: &mut Vec<String>,
) -> Result<(Vec<u8>, std::collections::BTreeSet<String>), CodecError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).map_err(|error| {
        CodecError::malformed(format_args!("cannot open nested Protein ZIP: {error}"))
    })?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut patched = std::collections::BTreeSet::new();
    let mut total_inflated = 0_u64;
    for index in 0..archive.len() {
        // `zip` cannot expand or write a Zstandard member: see `crate::zip_write`.
        let zstd = crate::zip_write::is_zstd_member(&mut archive, index)?;
        let (name, options, mut bytes) = {
            let mut entry = if zstd {
                archive.by_index_raw(index)
            } else {
                archive.by_index(index)
            }
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot read nested Protein entry: {error}"))
            })?;
            let name = entry
                .name()
                .map_err(|error| {
                    CodecError::malformed(format_args!("invalid ZIP entry name: {error}"))
                })?
                .into_owned();
            let options = crate::zip_write::file_options(entry.compression());
            let declared_size = entry.size();
            total_inflated = total_inflated.checked_add(declared_size).ok_or_else(|| {
                CodecError::Malformed("Protein ZIP total inflated size overflows u64".into())
            })?;
            if total_inflated > crate::container::MAX_ARCHIVE_BYTES {
                return Err(CodecError::malformed(format_args!(
                    "Protein ZIP entries declare {total_inflated} inflated bytes; total limit is {}",
                    crate::container::MAX_ARCHIVE_BYTES
                )));
            }
            let bytes = if zstd {
                crate::zip_write::expand_zstd_member(
                    &mut entry,
                    &name,
                    cadmpeg_core::decode::u64_from_index(protein.len()),
                )?
            } else {
                crate::container::read_entry_bounded(&mut entry, declared_size, &name)?
            };
            (name, options, bytes)
        };
        let expanded = zstd.then(|| bytes.clone());
        if name.ends_with("AssetData/InstanceProperties.bin") {
            patch_instance_colors(protein, &mut bytes, edits, &mut patched, notes)?;
        }
        if let Some(expanded) = expanded {
            crate::zip_write::write_zstd_member(
                &mut zip,
                &mut archive,
                index,
                &expanded,
                &bytes,
                notes,
            )?;
            continue;
        }
        zip.start_file(name, options).map_err(|error| {
            CodecError::malformed(format_args!("cannot write nested Protein entry: {error}"))
        })?;
        zip.write_all(&bytes)?;
    }
    let bytes = zip
        .finish()
        .map_err(|error| CodecError::malformed(format_args!("cannot finish Protein ZIP: {error}")))?
        .into_inner();
    Ok((bytes, patched))
}

fn patch_instance_colors(
    protein: &[u8],
    bytes: &mut [u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
    patched: &mut std::collections::BTreeSet<String>,
    notes: &mut Vec<String>,
) -> Result<(), CodecError> {
    let frames = cadmpeg_protein::framing::record_frames(bytes)?;
    let schema_driven = cadmpeg_protein::has_schemas(protein);
    let decoded = if schema_driven {
        let outcome = cadmpeg_protein::decode_frames_for_edit(protein, &frames)?;
        notes.extend(outcome.rejected.iter().map(|rejected| {
            format!(
                "Protein record {} rejected: {}",
                rejected.ordinal, rejected.detail
            )
        }));
        outcome.records
    } else {
        Vec::new()
    };
    for frame in frames {
        let record = frame.bytes();
        let mut position = RECORD_MARKER.len();
        let schema = take_lp_utf8(record, &mut position).ok_or_else(|| {
            CodecError::Malformed("Protein appearance schema is truncated".into())
        })?;
        let guid = take_lp_utf8(record, &mut position)
            .ok_or_else(|| CodecError::Malformed("Protein appearance GUID is truncated".into()))?;
        skip_lp_u32_bytes(record, &mut position).ok_or_else(|| {
            CodecError::Malformed(
                "Protein appearance record is truncated at the string after its GUID".into(),
            )
        })?;
        skip_lp_u32_bytes(record, &mut position).ok_or_else(|| {
            CodecError::Malformed(
                "Protein appearance record is truncated at the second string after its GUID".into(),
            )
        })?;
        let Some(edit) = edits.get(&guid) else {
            continue;
        };
        let decoded_record = if schema_driven {
            Some(
                decoded
                    .iter()
                    .find(|decoded| {
                        decoded.logical_offset == frame.logical_offset()
                            && decoded.schema == schema
                            && decoded.guid == guid
                    })
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "Protein appearance {guid} has no decoded schema record"
                        ))
                    })?,
            )
        } else {
            None
        };
        if let Some(color) = edit.color {
            let relative = if let Some(decoded_record) = decoded_record {
                let property_id =
                    appearance_base_color_property_id(decoded_record).ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "Protein appearance {guid} has no schema-selected color carrier"
                        ))
                    })?;
                let property = decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        matches!(
                            property.value(),
                            Some(cadmpeg_protein::property::PropertyValue::Color(_))
                        )
                    })
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "Protein appearance {guid} has no {property_id} color carrier"
                        ))
                    })?;
                property.value_offset
            } else {
                match schema.as_str() {
                    "GenericSchema" => {
                        position
                            + 112
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
                    "PrismOpaqueSchema" | "PrismMetalSchema" => position + 8,
                    "PrismTransparentSchema" => position + 121,
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} has no writable color carrier"
                        )));
                    }
                }
            };
            for (ordinal, value) in [color.r(), color.g(), color.b(), color.a()]
                .into_iter()
                .enumerate()
            {
                patch_logical_f64(
                    bytes,
                    frame.logical_offset() + relative + ordinal * 8,
                    f64::from(value),
                )?;
            }
        }
        for (name, value) in &edit.properties {
            let relative = if let Some(decoded_record) = decoded_record {
                let property_id = match (schema.as_str(), name.as_str()) {
                    ("GenericSchema", "reflectivity_at_0deg") => "generic_reflectivity_at_0deg",
                    ("GenericSchema", "refraction_index") => "generic_refraction_index",
                    ("PrismOpaqueSchema", "surface_roughness") => "surface_roughness",
                    ("PrismTransparentSchema", "refraction_index") => {
                        "transparent_refraction_index"
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} property {name} has no writable carrier"
                        )));
                    }
                };
                decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        property
                            .value()
                            .and_then(cadmpeg_protein::property::PropertyValue::float_value)
                            .is_some()
                    })
                    .map(|property| property.value_offset)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "Protein appearance {guid} has no {property_id} scalar carrier"
                        ))
                    })?
            } else {
                match (schema.as_str(), name.as_str()) {
                    ("GenericSchema", "reflectivity_at_0deg") => {
                        position
                            + 175
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
                    ("GenericSchema", "refraction_index") => {
                        position
                            + 201
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
                    ("PrismOpaqueSchema", "surface_roughness") => {
                        find_from(record, b"\x0e\x20\x00\x00", position)
                            .map(|marker| marker + 4)
                            .ok_or_else(|| {
                                CodecError::Malformed("Protein roughness carrier is absent".into())
                            })?
                    }
                    ("PrismTransparentSchema", "refraction_index") => position + 169,
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} property {name} has no writable carrier"
                        )));
                    }
                }
            };
            patch_logical_f64(bytes, frame.logical_offset() + relative, *value)?;
        }
        patched.insert(guid);
    }
    Ok(())
}

fn patch_logical_f64(
    bytes: &mut [u8],
    logical_offset: usize,
    value: f64,
) -> Result<(), CodecError> {
    for (ordinal, byte) in value.to_le_bytes().into_iter().enumerate() {
        let physical = logical_to_physical(bytes, logical_offset + ordinal).ok_or_else(|| {
            CodecError::Malformed("Protein scalar offset is outside paged storage".into())
        })?;
        bytes[physical] = byte;
    }
    Ok(())
}

fn logical_to_physical(bytes: &[u8], logical_offset: usize) -> Option<usize> {
    let mut logical_start = 0usize;
    for (index, page) in bytes
        .get(STREAM_HEADER_LEN..)?
        .chunks_exact(PAGE_SIZE)
        .enumerate()
    {
        let (physical_in_page, length) = if page.get(4..8) == Some(RECORD_MARKER) {
            (4, PAGE_SIZE - 4)
        } else if page.get(4..8) == Some(CONTINUATION_MARKER) {
            (8, PAGE_SIZE - 8)
        } else if page.get(0..4) == Some(TERMINAL_MARKER) {
            (8, usize::from(View::u16_le_at(page, 4)?))
        } else {
            return None;
        };
        if logical_offset < logical_start + length {
            return Some(
                STREAM_HEADER_LEN + index * PAGE_SIZE + physical_in_page + logical_offset
                    - logical_start,
            );
        }
        logical_start += length;
    }
    None
}

/// Appearance assets and body bindings from one material decode.
///
/// Bindings follow the design-entity join backbone in
/// [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
#[derive(Default)]
pub(crate) struct DecodedMaterials {
    /// Merged appearance records, deduplicated by [`AppearanceId`].
    pub(crate) appearances: Vec<Appearance>,
    /// Body-to-appearance bindings resolved through ACT and Design body-map joins.
    pub(crate) bindings: Vec<AppearanceBinding>,
    /// Per-face appearance assignments awaiting the BREP face-attribute join.
    pub(crate) face_assignments: Vec<FaceAppearanceAssignment>,
    /// Whether the document serializes any body or face appearance assignment.
    ///
    /// Protein assets form a document-local appearance catalog and need not be
    /// assigned to topology. This distinguishes an unassigned catalog from an
    /// assignment that failed to resolve.
    pub(crate) has_topology_assignments: bool,
    /// Distance-valued texture properties omitted because their unit tag has
    /// no defined model-space conversion.
    pub(crate) untyped_distance_properties: usize,
    /// Rejected Protein record diagnostics.
    pub(crate) notes: Vec<String>,
}

/// Decode `.protein` assets and Design and ACT assignments without resolved
/// Design body-map bindings.
///
/// The [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)
/// Design body-map join is skipped. Use [`decode_with_body_bindings`] when the
/// resolved map pairs are available.
pub(crate) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<DecodedMaterials, CodecError> {
    decode_with_body_bindings(ctx, scan, &[])
}

/// Decode appearance assets and resolve body bindings through the ordered,
/// blob-qualified Design body-map pairs, closing the design-entity join
/// backbone in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
pub(crate) fn decode_with_body_bindings<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    body_bindings: &[DesignBodyBinding],
) -> Result<DecodedMaterials, CodecError> {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let mut untyped_distance_properties = 0usize;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::ProteinAssets))
    {
        let protein = scan.entry_view(&entry.name).ok_or_else(|| {
            CodecError::Malformed("protein archive entry missing from scan".into())
        })?;
        let Some(instance) = instance_properties(ctx, protein)? else {
            continue;
        };
        let record_frames = cadmpeg_protein::framing::record_frames(instance.window())?;
        let catalog = definition_catalog(ctx, protein)?;
        let schema_catalog = cadmpeg_protein::SchemaCatalog::load(ctx, protein)?;
        let mut appearances = if let Some(mut schema_catalog) = schema_catalog {
            let outcome = cadmpeg_protein::decode_frames(ctx, &mut schema_catalog, &record_frames)?;
            for rejected in &outcome.rejected {
                let note = format!(
                    "Protein {} record {} rejected: {}",
                    entry.name, rejected.ordinal, rejected.detail
                );
                notes.push(note);
            }
            let records = outcome.records;
            let (mut decoded, untyped_count) = appearances_from_schema_records(ctx, &records)?;
            untyped_distance_properties = untyped_distance_properties
                .checked_add(untyped_count)
                .ok_or_else(|| {
                    CodecError::Malformed("untyped material distance count overflows".into())
                })?;
            let decoded_ids = decoded
                .iter()
                .map(|appearance| appearance.id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let mut fixed = decode_fixed_logical_records(&record_frames)?;
            fixed.retain(|appearance| !decoded_ids.contains(appearance.id.as_str()));
            decoded.append(&mut fixed);
            decoded
        } else {
            decode_fixed_logical_records(&record_frames)?
        };
        for appearance in &mut appearances {
            if let Some(name) = appearance.name.as_deref() {
                let mut matches = catalog.iter().filter(|((asset_id, schema), _)| {
                    asset_id == name
                        && appearance
                            .schema
                            .as_ref()
                            .is_none_or(|expected| expected == schema)
                });
                if let Some(((_, schema), category)) = matches.next() {
                    if matches.next().is_none() {
                        appearance.schema = Some(schema.to_owned());
                        appearance.category =
                            (category.as_deref()).map(std::borrow::ToOwned::to_owned);
                    }
                }
            }
        }
        out.append(&mut appearances);
    }
    out.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    if let Some(pair) = out
        .windows(2)
        .find(|pair| pair[0].id == pair[1].id && pair[0] != pair[1])
    {
        return Err(CodecError::malformed(format_args!(
            "F3D appearance asset {} has conflicting payloads",
            pair[0].id
        )));
    }
    out.dedup_by(|a, b| a.id == b.id);
    let assignments = decode_design_assignments(scan)?;
    let act_channels = decode_act_channels(scan)?;
    let object_types = decode_design_object_types(scan)?;
    for assignment in &assignments {
        if appearance_for_assignment(&out, assignment)?.is_none() {
            out.push(Appearance {
                id: crate::ids::appearance_id(assignment.visual_guid.identity_key()),
                name: (assignment
                    .visual_preset
                    .as_ref()
                    .map(|field| field.value.as_str()))
                .map(std::borrow::ToOwned::to_owned),
                asset_guid: Some((*assignment.visual_guid).to_owned()),
                library_id: None,
                visual_guid: Some((*assignment.visual_guid).to_owned()),
                physical_token: (assignment
                    .physical_token
                    .as_ref()
                    .map(|field| field.value.as_str()))
                .map(std::borrow::ToOwned::to_owned),
                schema: None,
                category: None,
                base_color: None,
                properties: BTreeMap::new(),
                textures: Vec::new(),
            });
        }
    }
    for appearance in &mut out {
        if let Some(assignment) = assignments.iter().find(|assignment| {
            appearance.visual_guid.as_deref().is_some_and(|guid| {
                crate::design::presentation::visual_token(guid).is_some()
                    && guid.eq_ignore_ascii_case(&assignment.visual_guid)
            })
        }) {
            appearance.physical_token = (assignment
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()))
            .map(std::borrow::ToOwned::to_owned);
        }
    }
    let mut bindings = bind_bodies(
        &out,
        &assignments,
        &act_channels,
        &object_types,
        body_bindings,
    )?;
    let body_overrides = decode_body_appearance_overrides(ctx, scan, body_bindings)?;
    for over in &body_overrides {
        if bindings.iter().any(
            |binding| matches!(&binding.target, AppearanceTarget::Body(body) if body == &over.body),
        ) {
            continue;
        }
        let Some(appearance) = appearance_for_visual_token(&out, &over.visual_guid, None)? else {
            continue;
        };
        bindings.push(AppearanceBinding {
            id: crate::ids::body_appearance_binding_id(
                over.entity_suffix,
                over.visual_guid.identity_key(),
            ),
            target: AppearanceTarget::Body(over.body.clone()),
            appearance: appearance.id.clone(),
            source_entity_id: None,
            object_type: (object_types.get(&over.entity_suffix).map(String::as_str))
                .map(std::borrow::ToOwned::to_owned),
            visible: None,
            channels: named_act_channels(
                format_args!(
                    "f3d:appearance:body#{}:{}",
                    over.entity_suffix, over.visual_guid
                ),
                act_channels.get(&over.entity_suffix),
            )?,
        });
    }
    let face_assignments = decode_face_appearance_assignments(scan)?;
    let has_topology_assignments =
        !assignments.is_empty() || !body_overrides.is_empty() || !face_assignments.is_empty();
    Ok(DecodedMaterials {
        appearances: out,
        bindings,
        face_assignments,
        has_topology_assignments,
        untyped_distance_properties,
        notes,
    })
}

fn appearances_from_schema_records(
    ctx: &DecodeContext<'_>,
    records: &[cadmpeg_protein::DecodedRecord],
) -> Result<(Vec<Appearance>, usize), CodecError> {
    let mut textures = BTreeMap::new();
    let mut untyped_distance_properties = 0usize;
    for record in records {
        let texture = match texture_asset(ctx, record)? {
            TextureAssetResult::NotTexture => continue,
            TextureAssetResult::UnknownDistanceUnit { count } => {
                untyped_distance_properties = untyped_distance_properties
                    .checked_add(count)
                    .ok_or_else(|| {
                        CodecError::Malformed("untyped material distance count overflows".into())
                    })?;
                continue;
            }
            TextureAssetResult::Usable(texture) => *texture,
        };
        match textures.get(&texture.asset_guid) {
            None => {
                let key = texture.asset_guid.clone();
                textures.insert(key, texture);
            }
            Some(existing) if existing == &texture => {}
            Some(_) => {
                return Err(CodecError::malformed(format_args!(
                    "Protein texture asset {} has conflicting payloads",
                    texture.asset_guid
                )));
            }
        }
    }
    let mut appearances = Vec::new();
    for record in records.iter().filter(|record| {
        !matches!(
            record.schema.as_str(),
            "UnifiedBitmapSchema" | "BumpMapSchema"
        )
    }) {
        let mut properties = BTreeMap::new();
        let mut connected = Vec::new();
        for (id, property) in &record.properties {
            if let Some(value) = property
                .value()
                .and_then(cadmpeg_protein::property::PropertyValue::float_value)
            {
                properties.insert(neutral_property_name(id).to_owned(), value);
            }
            for guid in property.connections() {
                if let Some(texture) = textures.get(guid) {
                    let reference = texture.to_ref(ctx, id)?;
                    connected.push(reference);
                }
            }
        }
        connected.sort_by(|left, right| {
            left.slot
                .cmp(&right.slot)
                .then_with(|| left.asset_guid.cmp(&right.asset_guid))
        });
        let base_color = appearance_base_color(record)
            .map(|color| appearance_opacity(record, color))
            .transpose()?;
        let appearance = Appearance {
            id: crate::ids::appearance_id(appearance_key(&record.guid)?),
            name: Some(record.base.clone()),
            asset_guid: Some(record.guid.clone()),
            library_id: library_id(&record.asset_lib_id),
            visual_guid: if is_physical_schema(&record.schema) {
                None
            } else {
                Some(record.guid.clone())
            },
            physical_token: None,
            schema: Some(record.schema.clone()),
            category: None,
            base_color,
            properties: cadmpeg_core::text::named_entries(
                format_args!("f3d:design:appearance#{}", record.guid),
                properties,
            )?,
            textures: connected,
        };
        appearances.push(appearance);
    }
    Ok((appearances, untyped_distance_properties))
}

/// Resolve the one schema member that supplies an appearance's neutral base
/// colour. An enabled common tint replaces the shader family's primary colour;
/// a disabled or absent tint does not participate in selection.
fn appearance_base_color(record: &cadmpeg_protein::DecodedRecord) -> Option<Color> {
    color_property(record, appearance_base_color_property_id(record)?)
}

fn appearance_opacity(
    record: &cadmpeg_protein::DecodedRecord,
    color: Color,
) -> Result<Color, CodecError> {
    if record.schema != "GenericSchema" {
        return Ok(color);
    }
    let Some(property) = record.properties.get("generic_transparency") else {
        return Ok(color);
    };
    let transparency = property
        .value()
        .and_then(cadmpeg_protein::property::PropertyValue::float_value)
        .filter(|value| (0.0..=1.0).contains(&value.get()))
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Protein asset {} generic_transparency must be a scalar in [0, 1]",
                record.guid
            ))
        })?;
    f32_from_f64(1.0 - transparency.get())
        .and_then(|opacity| color.with_alpha(opacity))
        .ok_or_else(|| CodecError::Malformed("Protein opacity is outside [0, 1]".into()))
}

/// Select the serialized color carrier that represents the neutral base color.
fn appearance_base_color_property_id(
    record: &cadmpeg_protein::DecodedRecord,
) -> Option<&'static str> {
    if matches!(
        record
            .properties
            .get("common_Tint_toggle")
            .and_then(|property| property.value()),
        Some(cadmpeg_protein::property::PropertyValue::Boolean(true))
    ) {
        return Some("common_Tint_color");
    }

    let id = match record.schema.as_str() {
        "GenericSchema" => "generic_diffuse",
        "MetalSchema" => "metal_color",
        "MetallicPaintSchema" => "metallicpaint_base_color",
        "PlasticVinylSchema" => "plasticvinyl_color",
        "PrismLayeredSchema" => "layered_diffuse",
        "PrismMetalSchema" => "metal_f0",
        "PrismOpaqueSchema" => "opaque_albedo",
        "PrismTransparentSchema" => "transparent_color",
        // `PrismCommonSchema` supplies the common fallback used by derived
        // families that do not define one primary constant-colour member.
        _ if record.properties.contains_key("surface_albedo") => "surface_albedo",
        _ => return None,
    };
    Some(id)
}

fn color_property(record: &cadmpeg_protein::DecodedRecord, id: &str) -> Option<Color> {
    let cadmpeg_protein::property::PropertyValue::Color([r, g, b, a]) =
        record
            .properties
            .get(id)
            .and_then(|property| property.value())?
    else {
        return None;
    };
    decoded_color([r.get(), g.get(), b.get(), a.get()])
}

fn decoded_color(values: [f64; 4]) -> Option<Color> {
    values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .then(|| {
            Color::new(
                f32_from_f64(values[0])?,
                f32_from_f64(values[1])?,
                f32_from_f64(values[2])?,
                f32_from_f64(values[3])?,
            )
        })
        .flatten()
}

pub(crate) fn decode_design_assignments(
    scan: &ContainerScan,
) -> Result<Vec<DesignMaterialAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let body_map = crate::design::decode::body::body_bindings(bytes, &metadata)?;
        for presentation in
            crate::design::decode::presentation::body_presentations(bytes, &metadata)?
        {
            let Some(material) = presentation.material else {
                continue;
            };
            let crate::design::decode::presentation::BodyPresentationOwner::Named {
                entity_id,
                entity_id_offset,
            } = presentation.owner
            else {
                continue;
            };
            let Some(body_binding) =
                unique_body_map_pair(&body_map, presentation.entity_suffix, "material assignment")?
            else {
                continue;
            };
            let assignment = DesignMaterialAssignment {
                id: crate::ids::native_scoped_id(
                    &entry.name,
                    "material-assignment",
                    presentation.byte_offset,
                ),
                asm_body_key: body_binding.asm_key,
                asm_body_key_offset: u64_from_index(body_binding.asm_key_offset),

                entity_suffix_offset: u64_from_index(body_binding.entity_suffix_offset()),
                entity_id,
                entity_id_offset,
                visual_guid: material.visual_guid,
                visual_guid_offset: material.visual_guid_offset,
                physical_token: Some(crate::records::identity::RecordedValue {
                    value: material.physical_token,
                    offset: material.physical_token_offset,
                }),
                visual_preset: material.visual_preset.map(|field| {
                    crate::records::identity::RecordedValue {
                        value: field.value,
                        offset: field.offset,
                    }
                }),
            };
            out.push(assignment);
        }
    }
    Ok(out)
}

/// One per-body appearance override joined through its exact Design body-map pair.
struct BodyAppearanceOverride {
    /// Solved body selected by the exact blob-qualified body-map pair.
    body: BodyId,
    /// The body's design-entity suffix.
    entity_suffix: u64,
    /// Complete serialized visual token bound by the body record.
    visual_guid: DesignVisualToken,
}

/// Decode per-body appearance overrides from browser body records in every
/// Design `BulkStream` and join them through the exact BREP body-map pair
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn decode_body_appearance_overrides(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    body_bindings: &[DesignBodyBinding],
) -> Result<Vec<BodyAppearanceOverride>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let body_map = crate::design::decode::body::body_bindings(bytes, &metadata)?;
        let mut appearances = browser_body_appearances(ctx, bytes)?;
        for presentation in
            crate::design::decode::presentation::body_presentations(bytes, &metadata)?
        {
            if presentation.owner
                != crate::design::decode::presentation::BodyPresentationOwner::Bare
                || presentation.browser_node.is_none()
            {
                continue;
            }
            let Some(material) = presentation.material else {
                continue;
            };
            appearances.push((presentation.entity_suffix, material.visual_guid));
        }
        for (entity_suffix, visual_guid) in appearances {
            let Some(map_pair) =
                unique_body_map_pair(&body_map, entity_suffix, "browser body appearance")?
            else {
                continue;
            };
            let Some(body) = resolved_body_for_map_pair(
                body_bindings,
                &crate::ids::native_design_body_binding_id(&entry.name, map_pair.asm_key_offset),
                map_pair.asm_key,
                u64_from_index(map_pair.asm_key_offset),
                map_pair.entity_suffix,
                u64_from_index(map_pair.entity_suffix_offset()),
            )?
            else {
                continue;
            };
            out.push(BodyAppearanceOverride {
                body,
                entity_suffix,
                visual_guid,
            });
        }
    }
    out.sort_by(|left, right| {
        left.body
            .cmp(&right.body)
            .then_with(|| left.entity_suffix.cmp(&right.entity_suffix))
            .then_with(|| left.visual_guid.cmp(&right.visual_guid))
    });
    out.dedup_by(|left, right| {
        left.body == right.body
            && left.entity_suffix == right.entity_suffix
            && left.visual_guid.matches(&right.visual_guid)
    });
    Ok(out)
}

/// One per-face appearance assignment from a Design `BulkStream`.
///
/// The face GUID joins the BREP face that carries the same GUID in its
/// `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceAppearanceAssignment {
    /// The face GUID shared with the BREP face attribute.
    pub(crate) face_guid: String,
    /// Complete serialized visual token bound by the face record.
    pub(crate) visual_guid: DesignVisualToken,
    /// Face-local neutral color carried by a legacy assignment entry.
    pub(crate) color: Option<Color>,
}

/// Decode per-face appearance assignments from every Design `BulkStream`.
///
/// A legacy face assignment ends with the `BA5EE55E-…` marker GUID. A current
/// assignment ends with the paired-library tail decoded by
/// [`modern_face_appearance_assignments`]. Both forms stay inside one exact
/// primary-index frame
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
fn decode_face_appearance_assignments(
    scan: &ContainerScan,
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        for frame in crate::metastream::primary_record_frames(&metadata, bytes.len())? {
            out.extend(face_appearance_assignments_in_frame(
                &bytes[frame.start..frame.end],
            ));
        }
    }
    Ok(out)
}

/// Decode a synthetic test slice as one Design primary-index frame.
#[cfg(test)]
fn face_appearance_assignments(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    face_appearance_assignments_in_frame(bytes)
}

/// Decode face assignments from one exact Design primary-index frame.
fn face_appearance_assignments_in_frame(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    let strings = lp_utf16_strings(bytes);
    let mut out = legacy_face_appearance_assignments(bytes, &strings);
    out.extend(modern_face_appearance_assignments(bytes, &strings));
    out
}

/// Decode the variable-width legacy face-assignment envelope.
///
/// Every accepted member is adjacent to the next one. This excludes other
/// body-presentation records that share the appearance-library marker.
fn legacy_face_appearance_assignments(
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Vec<FaceAppearanceAssignment> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const COLOR_BYTES: usize = 4 * size_of::<f32>();
    const CARRIER_BYTES: usize = 12;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in strings.iter().enumerate() {
        if marker != APPEARANCE_LIBRARY_ID {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(bytes, *visual_at) else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) {
            continue;
        }

        let visual_text = visual.to_owned();
        let Ok(visual) = DesignVisualToken::try_from(visual_text) else {
            continue;
        };
        let Some(face_at) = visual_at.checked_sub(LP_GUID_BYTES + COLOR_BYTES + CARRIER_BYTES)
        else {
            continue;
        };
        let Some((face_guid, face_len)) = lp_utf16_string_at(bytes, face_at) else {
            continue;
        };
        if face_len != LP_GUID_BYTES || !is_lowercase_guid(&face_guid) {
            continue;
        }
        let color_at = face_at + face_len;
        let Some(color) = normalized_legacy_face_color(bytes, color_at) else {
            continue;
        };
        let carrier_at = color_at + COLOR_BYTES;
        let Some(selector_kind) =
            legacy_face_selector_kind(bytes.get(carrier_at..carrier_at + CARRIER_BYTES))
        else {
            continue;
        };
        if carrier_at + CARRIER_BYTES != *visual_at {
            continue;
        }

        let Some((_, marker_len)) = lp_utf16_string_at(bytes, *marker_at) else {
            continue;
        };
        let mut cursor = marker_at + marker_len;
        let Some(optional_name_count) = View::u32_le_at(bytes, cursor) else {
            continue;
        };
        if optional_name_count == 0 {
            cursor += 4;
        } else {
            let Some((display_name, display_name_end)) = lp_utf16_bounded(bytes, cursor, 1..=256)
            else {
                continue;
            };
            if display_name.chars().any(char::is_control) {
                continue;
            }
            cursor = display_name_end;
        }
        let Some((selector, selector_len)) = lp_utf16_string_at(bytes, cursor) else {
            continue;
        };
        if !legacy_face_selector_is_valid(selector_kind, &selector) {
            continue;
        }
        cursor += selector_len;
        if bytes.get(cursor..cursor + 4) != Some(&0_f32.to_le_bytes())
            || bytes.get(cursor + 4..cursor + 8) != Some(&1_f32.to_le_bytes())
        {
            continue;
        }

        out.push(FaceAppearanceAssignment {
            face_guid,
            visual_guid: visual,
            color: Some(color),
        });
    }
    out
}

/// Decode the normalized RGBA carrier of a legacy face assignment.
fn normalized_legacy_face_color(bytes: &[u8], offset: usize) -> Option<Color> {
    let raw = bytes.get(offset..offset + 4 * size_of::<f32>())?;
    let component = |at: usize| View::f32_le_at(raw, at);
    let color = Color::new(component(0)?, component(4)?, component(8)?, component(12)?)?;
    (color.a() == 1.0).then_some(color)
}

/// Decode the selector-name form flag in the legacy twelve-byte carrier.
fn legacy_face_selector_kind(carrier: Option<&[u8]>) -> Option<u8> {
    let carrier = carrier?;
    (carrier.len() == 12
        && carrier.get(0..2) == Some(&[1, 1])
        && carrier.get(2..11) == Some(&[0; 9])
        && matches!(carrier[11], 0 | 1))
    .then(|| carrier[11])
}

/// Validate the selector family selected by the legacy carrier flag.
fn legacy_face_selector_is_valid(kind: u8, selector: &str) -> bool {
    match kind {
        0 => selector.strip_prefix("Prism-").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        }),
        1 => selector.strip_prefix("Prism").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }),
        _ => false,
    }
}

/// Decode a face-scoped appearance assignment from the paired-library marker
/// form.
///
/// The assignment envelope ends with the visual token and the two library
/// marker GUIDs. Two lower-case GUIDs precede the tail through fixed carrier
/// gaps; the second is the B-rep face-attribute identity. Other paired-library
/// envelopes do not satisfy this grammar.
fn modern_face_appearance_assignments(
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Vec<FaceAppearanceAssignment> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const FIRST_GUID_GAP: usize = 25;
    const FACE_GUID_GAP: usize = 28;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in strings.iter().enumerate() {
        if marker != APPEARANCE_LIBRARY_ID_PAIR[0]
            || strings
                .get(index + 1)
                .is_none_or(|(_, next)| next != APPEARANCE_LIBRARY_ID_PAIR[1])
        {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(bytes, *visual_at) else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) {
            continue;
        }

        let visual_text = visual.to_owned();
        let Ok(visual) = DesignVisualToken::try_from(visual_text) else {
            continue;
        };
        let Some((_, first_library_len)) = lp_utf16_string_at(bytes, *marker_at) else {
            continue;
        };
        let Some(second_library_at) = marker_at.checked_add(first_library_len).and_then(|end| {
            let separator_end = end.checked_add(4)?;
            (bytes.get(end..separator_end) == Some(&[0; 4])).then_some(separator_end)
        }) else {
            continue;
        };
        if strings.get(index + 1).map(|(at, _)| *at) != Some(second_library_at) {
            continue;
        }

        let Some(face_end) = visual_at.checked_sub(FACE_GUID_GAP) else {
            continue;
        };
        let Some(face_at) = face_end.checked_sub(LP_GUID_BYTES) else {
            continue;
        };
        let Some((face_guid, face_len)) = lp_utf16_string_at(bytes, face_at) else {
            continue;
        };
        if face_at.checked_add(face_len) != Some(face_end)
            || !is_lowercase_guid(&face_guid)
            || !is_face_to_visual_gap(bytes.get(face_end..*visual_at))
        {
            continue;
        }

        let Some(first_guid_end) = face_at.checked_sub(FIRST_GUID_GAP) else {
            continue;
        };
        let Some(first_guid_at) = first_guid_end.checked_sub(LP_GUID_BYTES) else {
            continue;
        };
        let Some((first_guid, first_guid_len)) = lp_utf16_string_at(bytes, first_guid_at) else {
            continue;
        };
        if first_guid_at.checked_add(first_guid_len) != Some(first_guid_end)
            || !is_lowercase_guid(&first_guid)
            || !is_first_guid_to_face_gap(bytes.get(first_guid_end..face_at))
        {
            continue;
        }
        out.push(FaceAppearanceAssignment {
            face_guid,
            visual_guid: visual,
            color: None,
        });
    }
    out
}

/// Validate the fixed carrier gap after the first lower-case GUID of a paired
/// face-appearance envelope. Its leading eight-byte field is not framing; the
/// remaining bytes are invariant.
fn is_first_guid_to_face_gap(gap: Option<&[u8]>) -> bool {
    gap.is_some_and(|gap| {
        gap.len() == 25
            && gap.get(8..16) == Some(&[0; 8])
            && gap.get(16..20) == Some(&1_u32.to_le_bytes())
            && gap.get(20..22) == Some(&[1, 1])
            && gap.get(22..25) == Some(&[0; 3])
    })
}

/// Validate the fixed presentation tail between the B-rep face GUID and the
/// visual token.
fn is_face_to_visual_gap(gap: Option<&[u8]>) -> bool {
    gap.is_some_and(|gap| {
        gap.len() == 28
            && gap.get(0..12) == Some(&[0; 12])
            && gap.get(12..16) == Some(&1_f32.to_le_bytes())
            && gap.get(16..18) == Some(&[1, 1])
            && gap.get(18..28) == Some(&[0; 10])
    })
}

/// Whether the complete value is a lower-case hyphenated hexadecimal GUID.
fn is_lowercase_guid(value: &str) -> bool {
    value.len() == GUID_LEN
        && is_guid_prefix(value)
        && value[..GUID_LEN]
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase())
}

/// Decode legacy body-presentation records that identify their body through a
/// browser-node GUID rather than a typed body owner.
///
/// The terminating visual marker is shared with face-presentation records.
/// A record is body-owned only when exactly one GUID in its bounded prefix
/// resolves through a browser-node record to one Design entity suffix.
fn browser_body_appearances(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<(u64, DesignVisualToken)>, CodecError> {
    let nodes = crate::design::decode::body::scanned_browser_node_entities(bytes);
    let strings = lp_utf16_strings(bytes);
    let mut out = Vec::new();
    for (index, (_, marker)) in strings.iter().enumerate() {
        // The visual token is the string before the marker, so a marker at the
        // first string names none. The token's index is the proof the
        // candidate window needs, so it is carried rather than derived again.
        let Some(visual_index) = index.checked_sub(1) else {
            continue;
        };
        if marker != APPEARANCE_LIBRARY_ID {
            continue;
        }
        let visual = &strings[visual_index].1;
        let visual_text = visual.to_owned();
        let Ok(visual) = DesignVisualToken::try_from(visual_text) else {
            continue;
        };
        if let Some(entity_suffix) = body_node_candidate(&strings, visual_index, &nodes) {
            out.push((entity_suffix, visual));
        }
    }
    let mut keep = ctx.alloc_filled(out.len(), false, "mark F3D unique browser appearances")?;
    let mut seen = std::collections::HashSet::new();
    for (index, (entity_suffix, visual)) in out.iter().enumerate() {
        let text: &str = visual;
        if seen.contains(&(*entity_suffix, text)) {
            continue;
        }

        seen.insert((*entity_suffix, text));
        keep[index] = true;
    }
    drop(seen);
    let mut index = 0;
    out.retain(|_| {
        let retain = keep[index];
        index += 1;
        retain
    });
    Ok(out)
}

/// The browser-node entity the strings before one visual token name.
///
/// `visual_index` is the index of the visual token itself, which
/// `browser_body_appearances` proved is the string before the marker. The
/// candidate window is the strings between the appearance marker and that
/// token, so the token's own index bounds it and no index is derived twice.
fn body_node_candidate(
    strings: &[(usize, String)],
    visual_index: usize,
    nodes: &std::collections::HashMap<String, u64>,
) -> Option<u64> {
    const APPEARANCE_MARKER: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
    let marker = strings[..=visual_index]
        .iter()
        .rposition(|(_, value)| value == APPEARANCE_MARKER)?;
    // Include the three strings before the marker and every string after it.
    let mut first = None;
    for (_, (_, candidate)) in strings[..visual_index]
        .iter()
        .enumerate()
        .filter(|(ordinal, _)| *ordinal >= marker || ordinal.abs_diff(marker) <= 3)
    {
        let mut folded = String::new();
        folded.extend(
            candidate
                .chars()
                .map(|character| character.to_ascii_lowercase()),
        );
        let Some(&entity) = nodes.get(&folded) else {
            continue;
        };
        if first.is_some_and(|previous| previous != entity) {
            return None;
        }
        first = Some(entity);
    }
    first
}

/// Admit an appearance asset GUID as an appearance identity key.
fn appearance_key(guid: &str) -> Result<cadmpeg_ir::ids::IdentityKey, CodecError> {
    cadmpeg_ir::ids::IdentityKey::try_new(guid).map_err(|error| {
        CodecError::malformed(format_args!("F3D appearance identity is invalid: {error}"))
    })
}

fn bind_bodies(
    appearances: &[Appearance],
    assignments: &[DesignMaterialAssignment],
    act_channels: &std::collections::HashMap<u64, BTreeMap<String, String>>,
    object_types: &std::collections::HashMap<u64, String>,
    body_bindings: &[DesignBodyBinding],
) -> Result<Vec<AppearanceBinding>, CodecError> {
    let mut out = Vec::new();
    for assignment in assignments {
        let Some(body) = resolved_body_for_map_pair(
            body_bindings,
            &assignment.id,
            assignment.asm_body_key,
            assignment.asm_body_key_offset,
            assignment.entity_id.suffix(),
            assignment.entity_suffix_offset,
        )?
        else {
            continue;
        };
        let Some(appearance) = appearance_for_assignment(appearances, assignment)? else {
            continue;
        };
        out.push(AppearanceBinding {
            id: crate::ids::assignment_appearance_binding_id(
                assignment.entity_id.as_str(),
                assignment.visual_guid.identity_key(),
            )
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "F3D appearance binding identity is invalid: {error}"
                ))
            })?,
            target: AppearanceTarget::Body(body),
            appearance: appearance.id.clone(),
            source_entity_id: Some(assignment.entity_id.as_str().to_owned()),
            object_type: (object_types
                .get(&assignment.entity_id.suffix())
                .map(String::as_str))
            .map(std::borrow::ToOwned::to_owned),
            visible: None,
            channels: named_act_channels(
                format_args!(
                    "f3d:appearance:binding#{}:{}",
                    assignment.entity_id.as_str(),
                    assignment.visual_guid
                ),
                act_channels.get(&assignment.entity_id.suffix()),
            )?,
        });
    }
    Ok(out)
}

/// Resolve one Design assignment to a unique appearance record.
///
/// The complete visual token is authoritative. A present preset name is a
/// secondary identity only when no appearance carries that token.
pub(crate) fn appearance_for_assignment<'a>(
    appearances: &'a [Appearance],
    assignment: &DesignMaterialAssignment,
) -> Result<Option<&'a Appearance>, CodecError> {
    appearance_for_visual_token(
        appearances,
        &assignment.visual_guid,
        assignment
            .visual_preset
            .as_ref()
            .map(|field| field.value.as_str()),
    )
}

/// Resolve one complete serialized visual token to a unique appearance.
///
/// A preset name is an optional fallback for assignments whose visual token
/// names no decoded asset. Absence of a preset supplies no fallback identity.
pub(crate) fn appearance_for_visual_token<'a>(
    appearances: &'a [Appearance],
    serialized_token: &DesignVisualToken,
    fallback_name: Option<&str>,
) -> Result<Option<&'a Appearance>, CodecError> {
    let exact = unique_appearance(
        appearances.iter().filter(|appearance| {
            appearance.visual_guid.as_deref().is_some_and(|token| {
                DesignVisualToken::try_from(token.to_owned())
                    .is_ok_and(|token| serialized_token.matches(&token))
            })
        }),
        "visual token",
    )?;
    if exact.is_some() {
        return Ok(exact);
    }
    let Some(name) = fallback_name else {
        return Ok(None);
    };
    unique_appearance(
        appearances
            .iter()
            .filter(|appearance| appearance.name.as_deref() == Some(name)),
        "visual preset",
    )
}

fn unique_appearance<'a>(
    mut matches: impl Iterator<Item = &'a Appearance>,
    identity: &str,
) -> Result<Option<&'a Appearance>, CodecError> {
    let Some(appearance) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "F3D {identity} matches multiple appearance assets"
        )));
    }
    Ok(Some(appearance))
}

/// Resolve one material owner through its exact ordered body-map pair.
///
/// ASM keys are local to the pair's BREP basename.
fn resolved_body_for_map_pair(
    body_bindings: &[DesignBodyBinding],
    owner_id: &str,
    asm_body_key: u64,
    asm_body_key_offset: u64,
    entity_suffix: u64,
    entity_suffix_offset: u64,
) -> Result<Option<BodyId>, CodecError> {
    let owner_stream = crate::ids::native_stream(owner_id).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D material owner has no native stream: {owner_id}"
        ))
    })?;
    let mut matches = body_bindings.iter().filter(|binding| {
        crate::ids::native_stream(binding.id()) == Some(owner_stream)
            && binding.asm_body_key == asm_body_key
            && binding.asm_body_key_offset() == asm_body_key_offset
            && binding.entity_suffix == entity_suffix
            && binding.entity_suffix_offset() == entity_suffix_offset
    });
    let Some(binding) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "F3D material owner {owner_id} matches multiple exact body-map pairs"
        )));
    }
    Ok(binding.body.clone())
}

fn decode_design_object_types(
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, String>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Metastream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 8 <= bytes.len() {
            let Some((object_type, after_type)) = lp_ascii_printable(bytes, position)? else {
                position += 1;
                continue;
            };
            if !object_type.chars().all(char::is_alphabetic) {
                position += 1;
                continue;
            }
            let mut view = View::over_retained(&bytes[after_type..]);
            let Some(count) = view.u32_le() else {
                break;
            };
            if count > 200 {
                position += 1;
                continue;
            }
            if view.counted(u64::from(count), 8).is_none() {
                position += 1;
                continue;
            }
            for _ in 0..count {
                let Some(id) = view.u64_le() else {
                    break;
                };
                let value = object_type.clone();
                out.insert(id, value);
            }
            position = after_type + view.position();
        }
    }
    Ok(out)
}

fn decode_act_channels(
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, BTreeMap<String, String>>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_act_stream(entry))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 4 <= bytes.len() {
            let Some((tag, after_tag)) = lp_ascii_printable(bytes, position)? else {
                position += 1;
                continue;
            };
            if tag.len() != 3 || !tag.bytes().all(|byte| byte.is_ascii_digit()) {
                position += 1;
                continue;
            }
            let Some(count) = View::u32_le_at(bytes, after_tag + 14) else {
                break;
            };
            if bytes.get(after_tag + 4..after_tag + 14) != Some(&[0u8; 10]) {
                position += 1;
                continue;
            }
            if !(1..=8).contains(&count) {
                position += 1;
                continue;
            }
            let mut cursor = after_tag + 18;
            let mut channels = BTreeMap::new();
            let mut valid = true;
            for _ in 0..count {
                let Some((name, after_name)) = lp_ascii_printable(bytes, cursor)? else {
                    valid = false;
                    break;
                };
                let Some((guid, after_guid)) = lp_utf16_bounded(bytes, after_name, 1..=64) else {
                    valid = false;
                    break;
                };
                if guid.len() != 36 {
                    valid = false;
                    break;
                }
                channels.insert(name, guid);
                cursor = after_guid;
            }
            if valid {
                if let Some((entity, end)) = lp_utf16_bounded(bytes, cursor, 1..=64) {
                    if let Some(suffix) = entity_suffix(&entity) {
                        out.insert(suffix, channels);
                    }
                    position = end;
                    continue;
                }
            }
            position += 1;
        }
    }
    Ok(out)
}

fn entity_suffix(value: &str) -> Option<u64> {
    let (_, suffix) = value.split_once('_')?;
    suffix.parse().ok()
}

fn lp_utf16_strings(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + 4 <= bytes.len() {
        let Some(count) =
            View::u32_le_at(bytes, offset).and_then(|count| usize::try_from(count).ok())
        else {
            offset += 1;
            continue;
        };
        let Some(payload_at) = offset.checked_add(4) else {
            offset += 1;
            continue;
        };
        if !(2..=256).contains(&count) || !utf16_string_prefix_is_text(bytes, payload_at, count) {
            offset += 1;
            continue;
        }
        if let Some((value, record_len)) = lp_utf16_string_at(bytes, offset) {
            out.push((offset, value));
            offset += record_len;
        } else {
            offset += 1;
        }
    }
    out
}

/// Reject an unframed string candidate before decoding its full declared run.
///
/// Appearance streams contain several unrelated binary records, so scanning
/// every byte can encounter a plausible length word whose payload is not text.
/// Checking only the first four code units keeps the heuristic bounded while
/// accepting supplementary-plane characters whose surrogate pair spans the
/// prefix boundary. The full strict decode remains authoritative.
fn utf16_string_prefix_is_text(bytes: &[u8], payload_at: usize, count: usize) -> bool {
    let prefix_count = count.min(4);
    let mut high_surrogate = false;
    for ordinal in 0..prefix_count {
        let Some(unit_offset) = ordinal
            .checked_mul(2)
            .and_then(|delta| payload_at.checked_add(delta))
        else {
            return false;
        };
        let Some(unit) = View::u16_le_at(bytes, unit_offset) else {
            return false;
        };
        if high_surrogate && !(0xdc00..=0xdfff).contains(&unit) {
            return false;
        }
        match unit {
            0 => return false,
            0xd800..=0xdbff => high_surrogate = true,
            0xdc00..=0xdfff if !high_surrogate => return false,
            0xdc00..=0xdfff => high_surrogate = false,
            value if char::from_u32(u32::from(value)).is_some_and(|value| !value.is_control()) => {
                high_surrogate = false;
            }
            _ => return false,
        }
    }
    true
}

/// Decode one LP-UTF16 string at `offset`. Rejects a count outside 2..=256,
/// invalid UTF-16, or a control character.
fn lp_utf16_string_at(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let (value, end) = lp_utf16_bounded(bytes, offset, 2..=256)?;
    if value.chars().any(char::is_control) {
        return None;
    }
    Some((value, end - offset))
}

/// Select the sole ordered body-map pair carrying one material owner's Design
/// entity suffix. More than one pair leaves the owner ambiguous.
fn unique_body_map_pair<'a>(
    body_map: &'a [crate::design::decode::body::BodyBinding],
    entity_suffix: u64,
    owner_kind: &str,
) -> Result<Option<&'a crate::design::decode::body::BodyBinding>, CodecError> {
    let mut matches = body_map
        .iter()
        .filter(|binding| binding.entity_suffix == entity_suffix);
    let Some(binding) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "F3D {owner_kind} entity {entity_suffix} matches multiple body-map pairs"
        )));
    }
    Ok(Some(binding))
}

/// Open a nested Protein ZIP member through the session archive expander so
/// per-expand and cumulative decompressed ceilings bind.
fn instance_properties<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
) -> Result<Option<View<'a>>, CodecError> {
    nested_entry(ctx, protein, "AssetData/InstanceProperties.bin")
}

fn definition_catalog<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
) -> Result<std::collections::HashMap<(String, String), Option<String>>, CodecError> {
    let Some(entry) = nested_entry(ctx, protein, "AssetData/DefinitionIteratorProperties.bin")?
    else {
        return Ok(std::collections::HashMap::new());
    };
    let frames = cadmpeg_protein::framing::record_frames(entry.window())?;
    let mut definitions = std::collections::HashMap::new();
    for frame in &frames {
        let definition = decode_definition_catalog_record(frame.bytes())?;
        merge_definition_catalog_record(&mut definitions, definition);
    }

    let mut catalog = std::collections::HashMap::new();

    for ((asset_id, schema), definition) in definitions {
        catalog.insert((asset_id, schema), definition.category);
    }
    Ok(catalog)
}

#[derive(Debug, PartialEq, Eq)]
struct DefinitionCatalog {
    schema: String,
    asset_id: String,
    category: Option<String>,
}

fn merge_definition_catalog_record(
    definitions: &mut std::collections::HashMap<(String, String), DefinitionCatalog>,
    definition: DefinitionCatalog,
) {
    let key = (definition.asset_id.clone(), definition.schema.clone());
    match definitions.entry(key) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(definition);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().category != definition.category {
                entry.get_mut().category = None;
            }
        }
    }
}

fn decode_definition_catalog_record(record: &[u8]) -> Result<DefinitionCatalog, CodecError> {
    let malformed = malformed_definition_catalog_record;
    if !record.starts_with(RECORD_MARKER) {
        return Err(malformed("marker", 0));
    }
    let mut position = RECORD_MARKER.len();
    let schema =
        take_lp_utf8(record, &mut position).ok_or_else(|| malformed("schema", position))?;
    let flag = *record
        .get(position)
        .ok_or_else(|| malformed("flag", position))?;
    if flag > 1 {
        return Err(malformed("flag", position));
    }
    position += 1;
    let asset_id = take_lp_utf8(record, &mut position)
        .ok_or_else(|| malformed("asset identifier", position))?;
    take_lp_utf8(record, &mut position)
        .ok_or_else(|| malformed("base asset identifier", position))?;
    let version =
        View::u32_le_at(record, position).ok_or_else(|| malformed("format version", position))?;
    position += 4;
    if version > 3 {
        return Err(malformed("format version", position - 4));
    }
    let category = if version >= 2 {
        Some(take_lp_utf8(record, &mut position).ok_or_else(|| malformed("category", position))?)
    } else {
        None
    };
    if version >= 1 {
        take_lp_utf8(record, &mut position).ok_or_else(|| malformed("group", position))?;
    }
    if version == 3 {
        take_lp_utf8(record, &mut position).ok_or_else(|| malformed("subgroup", position))?;
    }
    take_lp_utf8(record, &mut position).ok_or_else(|| malformed("description", position))?;
    consume_catalog_strings(record, &mut position)?;
    consume_catalog_strings(record, &mut position)?;
    if record[position..].iter().any(|byte| *byte != 0) {
        return Err(malformed("trailing padding", position));
    }
    Ok(DefinitionCatalog {
        schema,
        asset_id,
        category,
    })
}

fn malformed_definition_catalog_record(_field: &str, _position: usize) -> CodecError {
    CodecError::Malformed("Protein definition catalog record is malformed".into())
}

fn consume_catalog_strings(record: &[u8], position: &mut usize) -> Result<(), CodecError> {
    let count = View::u32_le_at(record, *position)
        .ok_or_else(|| malformed_definition_catalog_record("string count", *position))?;
    *position += 4;
    let count = record
        .len()
        .checked_sub(*position)
        .and_then(|left| bounded_len(u64::from(count), 4, left))
        .ok_or_else(|| malformed_definition_catalog_record("string count", *position))?;
    for _ in 0..count {
        take_lp_utf8(record, position)
            .ok_or_else(|| malformed_definition_catalog_record("string", *position))?;
    }
    Ok(())
}

fn nested_entry<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
    suffix: &str,
) -> Result<Option<View<'a>>, CodecError> {
    let archive = match ArchiveSnapshot::new(ctx, protein) {
        Ok(archive) => archive,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    for entry in archive.entries() {
        if entry.name.ends_with(suffix) {
            return Ok(Some(archive.open(ctx, &entry.name)?));
        }
    }
    Ok(None)
}

/// Decode the fixed source-less layouts emitted by [`encode_protein`]. Native
/// Protein assets package schemas and use the schema-driven path instead.
fn decode_fixed_logical_records(
    frames: &[cadmpeg_protein::framing::RecordFrame],
) -> Result<Vec<Appearance>, CodecError> {
    let mut appearances = Vec::new();
    for frame in frames {
        if let Some(appearance) = decode_fixed_record(frame.bytes())? {
            appearances.push(appearance);
        }
    }
    Ok(appearances)
}

/// Decode one record of a fixed source-less layout.
///
/// The schema set here is exactly the set [`encode_protein`] emits, and the
/// offsets are that encoder's own layout rather than a property order stated by
/// the format. A record carrying any other schema declines: its member offsets
/// follow from the schema packaged beside it, which the schema-driven path
/// reads. That covers the `interior_model` subtypes with no fixed layout here,
/// `PrismLayeredSchema` and `PrismWoodSchema`, whose colour offset must not be
/// assumed from the opaque, metal, or transparent layouts.
fn decode_fixed_record(record: &[u8]) -> Result<Option<Appearance>, CodecError> {
    let mut position = RECORD_MARKER.len();
    let Some(schema) = take_lp_utf8(record, &mut position) else {
        return Ok(None);
    };
    let Some(guid) = take_lp_utf8(record, &mut position) else {
        return Ok(None);
    };
    let Some(base) = take_lp_utf8(record, &mut position) else {
        return Ok(None);
    };
    let Some(asset_lib_id) = take_lp_utf8(record, &mut position) else {
        return Ok(None);
    };
    let color = match schema.as_str() {
        "GenericSchema" => {
            let Some(delta) = generic_connection_delta(record, position) else {
                return Ok(None);
            };
            fixed_rgba(record, position + 112 + delta)
        }
        "PrismOpaqueSchema" | "PrismMetalSchema" => fixed_rgba(record, position + 8),
        "PrismTransparentSchema" => fixed_rgba(record, position + 121),
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => None,
        _ => return Ok(None),
    };
    let mut properties = BTreeMap::new();
    if schema == "GenericSchema" {
        let Some(delta) = generic_connection_delta(record, position) else {
            return Ok(None);
        };
        fixed_tagged_scalar(
            &mut properties,
            "reflectivity_at_0deg",
            record,
            position + 171 + delta,
        );
        fixed_tagged_scalar(
            &mut properties,
            "refraction_index",
            record,
            position + 197 + delta,
        );
    } else if schema == "PrismOpaqueSchema" {
        if let Some(marker) = find_from(record, b"\x0e\x20\x00\x00", position) {
            fixed_scalar(&mut properties, "surface_roughness", record, marker + 4);
        }
    } else if schema == "PrismTransparentSchema" {
        fixed_scalar(&mut properties, "refraction_index", record, position + 169);
    }
    let properties = cadmpeg_core::text::named_entries(
        format_args!("f3d:design:appearance#{guid}"),
        properties,
    )?;
    Ok(Some(Appearance {
        id: crate::ids::appearance_id(appearance_key(&guid)?),
        name: Some(base),
        asset_guid: Some(guid.clone()),
        library_id: library_id(&asset_lib_id),
        visual_guid: (!matches!(
            schema.as_str(),
            "PhysMatSchema"
                | "StructuralMetalSchema"
                | "StructuralPlasticSchema"
                | "ThermalSolidSchema"
        ))
        .then_some(guid),
        physical_token: None,
        schema: Some(schema),
        category: None,
        base_color: color,
        properties,
        textures: Vec::new(),
    }))
}

fn fixed_scalar(out: &mut BTreeMap<String, FiniteReal>, name: &str, bytes: &[u8], offset: usize) {
    if let Some(value) = View::f64_le_at(bytes, offset).and_then(FiniteReal::new) {
        out.insert(name.to_owned(), value);
    }
}

fn fixed_tagged_scalar(
    out: &mut BTreeMap<String, FiniteReal>,
    name: &str,
    bytes: &[u8],
    offset: usize,
) {
    if bytes.get(offset..offset + 4) == Some(b"\x0c\x00\x00\x00") {
        fixed_scalar(out, name, bytes, offset + 4);
    }
}

fn fixed_rgba(bytes: &[u8], offset: usize) -> Option<Color> {
    let mut values = [0.0; 4];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let at = offset + ordinal * 8;
        *value = View::f64_le_at(bytes, at)?;
    }
    decoded_color(values)
}

fn generic_connection_delta(record: &[u8], value_block: usize) -> Option<usize> {
    let slot = value_block.checked_add(102)?;
    match record.get(slot) {
        Some(0) => Some(0),
        Some(1) if slot + 6 <= record.len() => {
            let count = index_from_u32(View::u32_le_at(record, slot + 2)?);
            if count > 8 {
                return None;
            }
            let mut position = slot + 6;
            for _ in 0..count {
                let length = index_from_u32(View::u32_le_at(record, position)?);
                position += 4;
                record.get(position..position + length)?;
                position += length;
            }
            position.checked_sub(slot + 1)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
