//! Phase 1 of the Design graph: the Design records that geometry and product
//! projection read.
//!
//! Design-intent decoding reads [`ProductPrereqs`] and never writes it.

use cadmpeg_core::{decode::DecodeContext, CodecError};

use crate::container::{self, BrepFacts, ContainerScan};
use crate::design::decode::parameters::UnparsedParameterRecords;
use crate::native::F3dNative;
use crate::records::{
    bodies::{DesignBodyBinding, DesignBodyMember},
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, DesignFeatureTimeline, SegmentType},
    feature::{assembly_features::DesignComponentOccurrence, scope::DesignParameterScope},
    parameters::{DesignParameter, DesignParameterOwner},
    recipes::{ConstructionRecipe, DesignComponentNamingSpace},
    references::DesignMaterialAssignment,
};
use crate::xref::XrefTable;

/// Design records that geometry and product projection read.
#[derive(Debug)]
pub(super) struct ProductPrereqs {
    pub(super) construction_recipes: Vec<ConstructionRecipe>,
    pub(super) design_material_assignments: Vec<DesignMaterialAssignment>,
    pub(super) design_types: Vec<SegmentType>,
    pub(super) design_parameters: Vec<DesignParameter>,
    /// Per-stream counts of typed parameter frames that match no parameter
    /// grammar; the session reports them as losses.
    pub(super) design_parameter_unparsed: Vec<UnparsedParameterRecords>,
    /// Native parameter identities retained without an interpreted owner frame.
    pub(super) design_parameter_owner_unparsed: Vec<String>,
    /// Non-equivalent duplicate scope envelopes superseded by the
    /// primary-indexed envelope; the session reports them as a loss.
    pub(super) design_superseded_scope_envelopes: usize,
    pub(super) design_entity_headers: Vec<DesignEntityHeader>,
    /// Headers of the records that entity headers index. Sketch relations
    /// decode from this set only.
    pub(super) design_record_headers: Vec<DesignRecordHeader>,
    /// Headers of the parameter-owner records that entity headers do not
    /// index.
    pub(super) design_owner_record_headers: Vec<DesignRecordHeader>,
    pub(super) design_parameter_owners: Vec<DesignParameterOwner>,
    pub(super) design_component_occurrences: Vec<DesignComponentOccurrence>,
    pub(super) design_superseded_component_occurrences: usize,
    /// Component-path carriers whose elements are all local records.
    pub(super) design_component_paths: Vec<crate::records::component_path::DesignComponentPath>,
    /// Local occurrences that two or more component-path carriers name.
    pub(super) design_ambiguous_current_poses: usize,
    pub(super) design_component_occurrence_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Scopes after history-bound variant admission, before design-intent
    /// binding.
    pub(super) design_parameter_scopes: Vec<DesignParameterScope>,
    pub(super) design_parameter_scope_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    /// Freedoms and limits of the joints whose motion records carry them.
    pub(super) design_joint_motions: Vec<crate::records::feature::joint_motion::DesignJointMotion>,
    /// Authored feature order, which tells top-level `Component Insert`
    /// scopes from internal operands. A decode failure that does not refuse
    /// the document stays here; design intent then does not run.
    pub(super) design_feature_timelines: Result<Vec<DesignFeatureTimeline>, CodecError>,
    pub(super) design_component_naming_spaces: Vec<DesignComponentNamingSpace>,
    /// Registered component entities with no context-UUID binding in this
    /// document; the session reports them as a loss.
    pub(super) design_unbound_component_naming_spaces: usize,
    pub(super) design_body_bindings: Vec<DesignBodyBinding>,
    pub(super) design_body_members: Vec<DesignBodyMember>,
    /// Retained-byte allowance for ASM history binding, taken before phase 1.
    pub(super) history_retained_allowance: u64,
    /// Container reference table. A parse failure that does not refuse the
    /// document stays here for the product report.
    pub(super) xref: Result<Option<XrefTable>, CodecError>,
}

impl ProductPrereqs {
    /// Container reference table, when it was decoded.
    pub(super) fn xref_table(&self) -> Option<&XrefTable> {
        self.xref.as_ref().ok().and_then(Option::as_ref)
    }

    /// Authored feature timelines, when they were decoded.
    pub(super) fn feature_timelines(&self) -> Option<&[DesignFeatureTimeline]> {
        self.design_feature_timelines.as_deref().ok()
    }

    /// Every phase-1 record header, sorted by identity.
    pub(super) fn record_headers(&self) -> Vec<DesignRecordHeader> {
        merged_record_headers(
            &self.design_record_headers,
            &self.design_owner_record_headers,
        )
    }

    /// Move every Design record into native storage and return the container
    /// reference outcome.
    pub(super) fn store(self, native: &mut F3dNative) -> Result<Option<XrefTable>, CodecError> {
        let Self {
            construction_recipes,
            design_material_assignments,
            design_types,
            design_parameters,
            design_parameter_unparsed: _,
            design_parameter_owner_unparsed: _,
            design_superseded_scope_envelopes: _,
            design_entity_headers,
            design_record_headers,
            design_owner_record_headers,
            design_parameter_owners,
            design_component_occurrences,
            design_superseded_component_occurrences: _,
            design_component_paths,
            design_ambiguous_current_poses: _,
            design_component_occurrence_losses: _,
            design_parameter_scopes,
            design_parameter_scope_losses: _,
            design_joint_motions,
            design_feature_timelines,
            design_component_naming_spaces,
            design_unbound_component_naming_spaces: _,
            design_body_bindings,
            design_body_members,
            history_retained_allowance: _,
            xref,
        } = self;
        if let Ok(timelines) = design_feature_timelines {
            native.design_feature_timelines = timelines;
        }
        native.construction_recipes = construction_recipes;
        native.design_material_assignments = design_material_assignments;
        native.design_types = design_types;
        native.design_parameters = design_parameters;
        native.design_entity_headers = design_entity_headers;
        native.design_record_headers =
            merged_record_headers(&design_record_headers, &design_owner_record_headers);
        native.design_parameter_owners = design_parameter_owners;
        native.design_component_occurrences = design_component_occurrences;
        native.design_component_paths = design_component_paths;
        native.design_parameter_scopes = design_parameter_scopes;
        native.design_joint_motions = design_joint_motions;
        native.design_component_naming_spaces = design_component_naming_spaces;
        native.design_body_bindings = design_body_bindings;
        native.design_body_members = design_body_members;
        xref
    }
}

/// Decode the product prerequisites of one Design document.
pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    design_body_bindings: Vec<DesignBodyBinding>,
) -> Result<ProductPrereqs, CodecError> {
    let history_retained_allowance = ctx.retained_allowance();
    // Scope-variant admission reads state identities and chain links only.
    let mut history_index = Vec::new();
    for history_brep in container::history_breps(scan) {
        let bytes = scan.entry_bytes(&history_brep.name)?;
        if let Some(history) = crate::history::decode_header_index(
            bytes,
            &history_brep.name,
            history_ref_width(history_brep),
        ) {
            history_index.push(history);
        }
    }
    let construction_recipes = crate::design::decode::parameters::decode_recipes(scan)?;
    let design_material_assignments = crate::materials::decode_design_assignments(scan)?;
    let design_types = crate::design::decode::meta::decode_types(scan)?;
    let crate::design::decode::parameters::DecodedParameters {
        parameters: design_parameters,
        unparsed: design_parameter_unparsed,
    } = crate::design::decode::parameters::decode_parameters(scan)?;
    let design_entity_headers = crate::design::decode::sketch::decode_entity_headers(scan)?;
    let design_record_headers =
        crate::design::decode::sketch::decode_record_headers(scan, &design_entity_headers)?;
    let design_owner_record_headers = related_record_headers(
        scan,
        &design_record_headers,
        &parameter_owner_indices(&design_parameters),
    )?;
    let crate::design::decode::parameters::DecodedParameterOwners {
        owners: design_parameter_owners,
        unparsed: design_parameter_owner_unparsed,
    } = crate::design::decode::parameters::decode_parameter_owners(
        scan,
        &design_parameters,
        &merged_record_headers(&design_record_headers, &design_owner_record_headers),
    )?;
    let crate::design::decode::components::DecodedComponentOccurrences {
        occurrences: design_component_occurrences,
        superseded: design_superseded_component_occurrences,
        ambiguous_current_poses: design_ambiguous_current_poses,
        losses: design_component_occurrence_losses,
        paths: design_component_paths,
    } = crate::design::decode::components::decode_component_occurrences(scan)?;
    let crate::design::decode::scopes::parameter_scope::DecodedParameterScopes {
        scopes: mut design_parameter_scopes,
        losses: design_parameter_scope_losses,
    } = crate::design::decode::scopes::parameter_scope::decode_parameter_scopes(
        ctx,
        scan,
        &crate::design::decode::scopes::parameter_scope::ParameterScopeSources {
            entities: &design_entity_headers,
            types: &design_types,
            parameters: &design_parameters,
            parameter_owners: &design_parameter_owners,
            component_occurrences: &design_component_occurrences,
            recipes: &construction_recipes,
        },
    )?;
    let primary_frames =
        crate::design::decode::scopes::parameter_scope::design_primary_frame_offsets(scan)?;
    let design_superseded_scope_envelopes =
        crate::design::decode::scopes::parameter_scope::admit_history_bound_scope_variants(
            ctx,
            &mut design_parameter_scopes,
            &history_index,
            &primary_frames,
        )?;
    let design_joint_motions =
        crate::design::decode::joint_motion::decode_joint_motions(scan, &design_parameter_scopes)?;
    let design_feature_timelines = match crate::design::decode::meta::decode_feature_timelines(scan)
    {
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        other => other,
    };
    let crate::design::decode::meta::DecodedComponentNamingSpaces {
        spaces: design_component_naming_spaces,
        unbound: design_unbound_component_naming_spaces,
    } = crate::design::decode::meta::decode_component_naming_spaces(scan)?;
    let design_body_members = crate::design::decode::body::decode_body_members(scan)?;
    let xref = match crate::xref::decode_with_scopes(ctx, scan, &design_parameter_scopes) {
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::NotImplemented(_))) => {
            return Err(error)
        }
        other => other,
    };
    Ok(ProductPrereqs {
        construction_recipes,
        design_material_assignments,
        design_types,
        design_parameters,
        design_parameter_unparsed,
        design_parameter_owner_unparsed,
        design_superseded_scope_envelopes,
        design_entity_headers,
        design_record_headers,
        design_owner_record_headers,
        design_parameter_owners,
        design_component_occurrences,
        design_superseded_component_occurrences,
        design_component_paths,
        design_ambiguous_current_poses,
        design_component_occurrence_losses,
        design_parameter_scopes,
        design_parameter_scope_losses,
        design_joint_motions,
        design_feature_timelines,
        design_component_naming_spaces,
        design_unbound_component_naming_spaces,
        design_body_bindings,
        design_body_members,
        history_retained_allowance,
        xref,
    })
}

/// Reference width of one ASM history stream.
pub(super) fn history_ref_width(history_brep: &BrepFacts) -> cadmpeg_asm::kernel_header::RefWidth {
    history_brep
        .kernel
        .as_ref()
        .and_then(crate::container::KernelFraming::asm_header)
        .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
            header.width
        })
}

/// Owner record of every Design parameter.
fn parameter_owner_indices(parameters: &[DesignParameter]) -> Vec<(String, u32)> {
    collect_related_indices(parameters.iter().filter_map(|parameter| {
        Some((
            crate::ids::native_stream(&parameter.id)?,
            parameter.owner_record_index()?,
        ))
    }))
}

pub(super) fn collect_related_indices<'a>(
    indices: impl IntoIterator<Item = (&'a str, u32)>,
) -> Vec<(String, u32)> {
    let mut collected = Vec::new();
    for (stream, index) in indices {
        let stream = stream.to_owned();
        collected.push((stream, index));
    }
    collected
}

/// Decode the headers of the related records that `existing` does not hold.
fn related_record_headers(
    scan: &ContainerScan,
    existing: &[DesignRecordHeader],
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let held = existing
        .iter()
        .filter_map(|record| Some((crate::ids::native_stream(&record.id)?, record.record_index)))
        .collect::<std::collections::HashSet<_>>();
    let mut wanted = Vec::new();
    for (stream, index) in indices {
        if !held.contains(&(stream.as_str(), *index)) {
            wanted.push((stream.clone(), *index));
        }
    }
    crate::design::decode::sketch::decode_related_record_headers(scan, &wanted)
}

/// Add the headers of related records that `headers` does not hold yet.
pub(super) fn append_related_record_headers(
    scan: &ContainerScan,
    headers: &mut Vec<DesignRecordHeader>,
    indices: &[(String, u32)],
) -> Result<(), CodecError> {
    let related = related_record_headers(scan, headers, indices)?;
    headers.extend(related);
    headers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// Two disjoint header sets as one, sorted by identity.
pub(super) fn merged_record_headers(
    first: &[DesignRecordHeader],
    second: &[DesignRecordHeader],
) -> Vec<DesignRecordHeader> {
    let mut headers = first.iter().chain(second).cloned().collect::<Vec<_>>();
    headers.sort_by(|a, b| a.id.cmp(&b.id));
    headers
}
