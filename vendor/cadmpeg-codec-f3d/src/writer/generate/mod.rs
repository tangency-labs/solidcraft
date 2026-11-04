// SPDX-License-Identifier: Apache-2.0
//! Source-less F3D archive generation: assemble a ZIP archive from a neutral
//! `CadIr` with no retained source.

use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom, Write};

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ArenaDisposition, ArenaDispositions};
use cadmpeg_ir::document::CadIr;

use crate::manifest::{self, GENERATED_DESIGN_ASSET_FOLDER as DESIGN_FOLDER};
use crate::writer::primitives::{
    f3d_native, validate_assembly_projection, validate_configuration_projection,
};
mod attributes;
mod index;
mod native_bytes;
pub(crate) mod native_geometry;
mod preconditions;
mod presentation;
mod records;
mod smbh;
use preconditions::{
    validate_source_less_auxiliary_geometry, validate_source_less_design_bindings,
    validate_source_less_design_links, validate_source_less_design_ownership,
    validate_source_less_history_graph, validate_source_less_procedural_carriers,
    validate_source_less_recipes, validate_source_less_sketch_graph,
    validate_source_less_topology_tolerances,
};
use presentation::GeneratedDesignRegistry;
use records::{encode_design_bulkstream, encode_design_metastream, encode_document_parameters};
use smbh::encode_smbh;

/// Write a canonical source-less F3D archive for the currently supported
/// native construction profile, and state which model arenas it carries.
pub(super) fn write_new(
    target: &CadIr,
    writer: &mut dyn Write,
) -> Result<ArenaDispositions, CodecError> {
    // `subset_int_cur` states its interval and cache on the parameter of the
    // curve it trims.
    let lowered = cadmpeg_ir::codec::write::subset_lowering::lower_subset_curves(target, |_| true)?;
    let target = lowered.as_ref();
    let loaded_native = f3d_native(target)?;
    validate_assembly_projection(target, loaded_native.as_ref())?;
    let has_native = loaded_native.is_some();
    let native = loaded_native.unwrap_or_default();
    let attributes = attributes::AttributeIndex::new(target, &native);
    if !target.model.subds.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation does not support SubD surfaces".into(),
        ));
    }
    if !target.model.assets.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation does not support document assets".into(),
        ));
    }
    validate_source_less_procedural_carriers(target)?;
    validate_source_less_topology_tolerances(target, &native)?;
    validate_source_less_auxiliary_geometry(target)?;
    if !native.act_entities.is_empty()
        || !native.act_guids.is_empty()
        || !native.act_registry_channels.is_empty()
        || !native.act_component_links.is_empty()
        || !native.act_table_references.is_empty()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D ACT generation requires a retained MetaStream record registry".into(),
        ));
    }
    let design_bindings = validate_source_less_design_bindings(&native)?;
    let mut parameter_bytes = Vec::new();
    if has_native {
        validate_configuration_projection(target, &native)?;
        validate_source_less_history_graph(target, &native)?;
        parameter_bytes = encode_document_parameters(&native.design_parameters)?;
        validate_source_less_design_ownership(&native)?;
        validate_source_less_sketch_graph(&native)?;
        validate_source_less_recipes(&native)?;
        validate_source_less_design_links(target, &native, &attributes)?;
    }
    let design_registry = GeneratedDesignRegistry::new(target, design_bindings, &attributes)?;
    let smbh = encode_smbh(target, &native, &attributes)?;
    let mut staged = tempfile::tempfile()?;
    let mut archive = zip::ZipWriter::new(&mut staged);
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    archive
        .start_file("Manifest.dat", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D manifest: {error}"))
        })?;
    archive.write_all(&manifest::generated_top_level()?)?;
    archive
        .start_file("Properties.dat", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D properties: {error}"))
        })?;
    archive.write_all(&0u32.to_le_bytes())?;
    archive
        .start_file(format!("{DESIGN_FOLDER}/Manifest.dat"), options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D asset manifest: {error}"))
        })?;
    archive.write_all(&manifest::generated_design_asset()?)?;
    archive
        .start_file(
            format!("{DESIGN_FOLDER}/Breps.BlobParts/BREP.generated.smbh"),
            options,
        )
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create F3D BREP entry: {error}"))
        })?;
    archive.write_all(&smbh)?;
    if has_native {
        let mut configuration_names = BTreeSet::new();
        for configuration in &native.design_configurations {
            if !configuration_names.insert(configuration.entry_name().as_str()) {
                return Err(CodecError::malformed(format_args!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name()
                )));
            }
            if configuration.active_row().is_some() {
                return Err(CodecError::NotImplemented(
                    "source-less F3D generation does not write the Design configuration root record"
                        .into(),
                ));
            }
            let payload =
                crate::records::configuration::encode_configuration_payload(configuration)?;
            archive
                .start_file(configuration.entry_name(), options)
                .map_err(|error| {
                    CodecError::malformed(format_args!(
                        "cannot create F3D configuration entry: {error}"
                    ))
                })?;
            archive.write_all(&payload)?;
        }
    }
    let design_bulk = encode_design_bulkstream(target, &native, &design_registry, parameter_bytes)?;
    if let Some(bulk_stream) = &design_bulk {
        archive
            .start_file(format!("{DESIGN_FOLDER}/Design1/BulkStream.dat"), options)
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Design BulkStream: {error}"))
            })?;
        archive.write_all(&bulk_stream.bytes)?;
    }
    let primary_records = design_bulk.as_ref().map_or(&[][..], |bulk_stream| {
        bulk_stream.primary_records.as_slice()
    });
    if let Some(meta_stream) = encode_design_metastream(&design_registry, primary_records)? {
        archive
            .start_file(format!("{DESIGN_FOLDER}/Design1/MetaStream.dat"), options)
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Design MetaStream: {error}"))
            })?;
        archive.write_all(&meta_stream)?;
    }
    for (ordinal, appearance) in target.model.appearances.iter().enumerate() {
        let protein = crate::materials::encode_protein(appearance)?;
        archive
            .start_file(
                format!("{DESIGN_FOLDER}/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"),
                options,
            )
            .map_err(|error| {
                CodecError::malformed(format_args!("cannot create F3D Protein asset: {error}"))
            })?;
        archive.write_all(&protein)?;
    }
    archive.finish().map_err(|error| {
        CodecError::malformed(format_args!("cannot finish F3D archive: {error}"))
    })?;
    staged.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut staged, writer)?;
    Ok(coverage(has_native))
}

/// What [`write_new`] does with each model arena.
/// `subds`, assets, tessellations and assembly joints are `Dropped` because
/// non-empty arenas are refused; parameters are also `Dropped` without native
/// Design records because their required projection is empty.
///
/// The generator writes B-rep topology, geometry, procedural carriers and
/// appearances from the neutral model. Configurations and parameters are
/// written from the native Design records after the neutral arenas are proved
/// equal to their projection; without an `f3d` namespace a non-empty parameter
/// arena is refused and configurations are not read. Design history, sketches
/// and the remaining document arenas come only from native records, so the
/// neutral arenas are not represented.
fn coverage(has_native: bool) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Omitted, Written};
    ArenaDispositions {
        bodies: Written,
        regions: Written,
        shells: Written,
        faces: Written,
        loops: Written,
        coedges: Written,
        edges: Written,
        vertices: Written,
        points: Written,
        surfaces: Written,
        curves: Written,
        subds: Dropped,
        pcurves: Written,
        procedural_surfaces: Written,
        procedural_curves: Written,
        assets: Dropped,
        features: Omitted,
        feature_input_topologies: Omitted,
        feature_result_topologies: Omitted,
        configurations: if has_native { Written } else { Omitted },
        parameters: if has_native { Written } else { Dropped },
        sketches: Omitted,
        sketch_entities: Omitted,
        sketch_constraints: Omitted,
        spatial_sketches: Omitted,
        spatial_sketch_entities: Omitted,
        spatial_sketch_constraints: Omitted,
        spreadsheets: Omitted,
        product_definitions: Omitted,
        occurrences: Omitted,
        assembly_joints: Dropped,
        drawings: Omitted,
        semantic_annotations: Omitted,
        presentation_documents: Omitted,
        view_presentations: Omitted,
        tessellations: Dropped,
        point_clouds: Omitted,
        appearances: Written,
        appearance_bindings: Omitted,
        attributes: Omitted,
        pmi: Omitted,
        presentation_layers: Omitted,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
    }
}
