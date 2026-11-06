// SPDX-License-Identifier: Apache-2.0
//! External-reference (`XRef`) and document-type entries of a `.f3d` container
//! ([spec §1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#12-stored-property-and-configuration-entries),
//! [§1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
//!
//! [`decode`] parses the top-level `RedirectionsStream.dat` table into
//! [`XrefDesign`] and [`XrefReference`] records. [`docstruct`] parses the JSON
//! form of `Properties.dat`. [`is_assembly`] classifies a BREP-less document
//! whose model is the placement of its XREF targets.

pub(crate) mod operation_pose;

use cadmpeg_core::container::ContainerRole;

use std::collections::HashSet;

use serde::Deserialize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};

use crate::bytes::utf16::Utf16View;
use crate::bytes::{
    is_guid_relaxed, lp_ascii_filtered_view, lp_ascii_strict, lp_utf16_bounded,
    lp_utf16_bounded_view, take_owned_reference, take_reference, Reference,
};
use crate::container::ContainerScan;
use crate::design::decode::components::ROOT_INSTANCE_NAME;
use crate::layout::component_insert_grouped_identity_carrier as grouped_identity_layout;
use crate::layout::legacy_attributed_external_placement_attributes as legacy_attributes;
use crate::layout::legacy_attributed_external_placement_empty_attributes as legacy_empty_attributes;
use crate::layout::legacy_attributed_external_placement_identity_control as legacy_identity_control;
use crate::layout::legacy_attributed_external_placement_identity_tail as legacy_identity_tail;
use crate::layout::legacy_attributed_external_placement_matrix_tail as legacy_matrix_tail;
use crate::layout::legacy_attributed_external_placement_members as legacy_members;
use crate::layout::legacy_attributed_external_placement_prefix as legacy_prefix;
use crate::layout::root_instance_prefix as root_instance;
use crate::records::{
    feature::{assembly_features::DesignComponentInsertConstruction, scope::DesignParameterScope},
    xref::{DerivedReference, XrefDesign, XrefLocalOwner, XrefPlacementSource, XrefReference},
};

/// Top-level container entry holding the external-reference table.
const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
const PROPERTIES_ENTRY: &str = "Properties.dat";
/// Top-level JSON document carrying component-reference extension data.
const COMPONENT_REFERENCE_ENTRY: &str = "ComponentReferenceData.json";

/// Stable type-table identity of a Design occurrence-placement record.
const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";
/// `CommonData` component-path carriers (spec §1.4).
pub(crate) const COMPONENT_PATH_TYPE_GUID: &str = "3B6814F4-D87B-46BD-AD9D-0BE07880F7BB";

/// The parsed `RedirectionsStream.dat` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct XrefTable {
    /// Design entries in source order; entry 0 is the document itself.
    pub(crate) designs: Vec<XrefDesign>,
    /// Outgoing XREF placements in source order; empty for a leaf document.
    pub(crate) references: Vec<XrefReference>,
    pub(crate) derived_references: Vec<DerivedReference>,
    pub(crate) unplaced_references: Vec<String>,
    /// Type-admitted placements that do not close under their generation grammar.
    pub(crate) placement_failures: Vec<OccurrencePlacementFailure>,
    pub(crate) unbound_legacy_placements: Vec<OccurrencePlacementFailure>,
    /// Placements named by two or more component-path carriers, as
    /// `(placement record, carrier records)`; they keep their placement matrix.
    pub(crate) ambiguous_current_poses: Vec<(u64, Vec<u64>)>,
    /// `(XREF ordinal, placement-record count)` pairs whose structured
    /// placements were superseded by scope-bound Component Insert carriers.
    pub(crate) placement_overrides: Vec<PlacementOverride>,
    pub(crate) placement_metadata_missing: bool,
}

/// One XREF ordinal whose structured placements were superseded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacementOverride {
    pub(crate) ordinal: u32,
    pub(crate) count: usize,
}

/// The `docstruct` document-type declaration of a JSON `Properties.dat`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Docstruct {
    /// Document type: `assembly-design` or `part-design`.
    pub(crate) doc_type: String,
    /// Document subtype, e.g. `assembly-standard` or `part-sheetmetal`.
    pub(crate) subtype: Option<String>,
}

#[derive(Deserialize)]
struct RedirectionsJson {
    name: String,
    #[serde(rename = "schema-version")]
    schema_version: u32,
    designs: Vec<DesignJson>,
    /// `{}` in a leaf document, an array in a referencing document.
    references: ReferencesJson,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum ReferencesJson {
    List(Vec<ReferenceJson>),
    /// The only legal non-array form is the empty leaf object `{}`.
    Leaf {},
}

#[derive(Deserialize)]
struct DesignJson {
    #[serde(rename = "file-version")]
    file_version: i64,
    #[serde(rename = "targetFileName")]
    target_file_name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "lineageUrn")]
    lineage_urn: String,
    #[serde(rename = "versionUrn")]
    version_urn: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ReferenceJson {
    #[serde(rename = "XREF")]
    Xref(ReferenceFieldsJson),
    #[serde(rename = "DERIVED")]
    Derived(ReferenceFieldsJson),
}

#[derive(Deserialize)]
struct ReferenceFieldsJson {
    from: String,
    #[serde(rename = "relativePath")]
    relative_path: String,
    properties: Vec<PropertyJson>,
}

#[derive(Deserialize)]
enum PropertyJson {
    #[serde(rename = "neutronRole")]
    Role(StringProperty),
    #[serde(rename = "neutronData")]
    Data(StringProperty),
}

#[derive(Deserialize)]
#[serde(tag = "dataType", deny_unknown_fields)]
enum StringProperty {
    #[serde(rename = "STRING")]
    String { value: String },
}

impl ReferenceFieldsJson {
    fn into_record(self, ordinal: usize) -> Result<XrefReference, CodecError> {
        let Self {
            from,
            relative_path,
            properties,
        } = self;
        let from = required_text(from, format_args!("references[{ordinal}].from"))?;
        let relative_path = required_text(
            relative_path,
            format_args!("references[{ordinal}].relativePath"),
        )?;
        let mut role = None;
        let mut data = None;
        for property in properties {
            let (name, value, slot) = match property {
                PropertyJson::Role(StringProperty::String { value }) => {
                    ("neutronRole", value, &mut role)
                }
                PropertyJson::Data(StringProperty::String { value }) => {
                    ("neutronData", value, &mut data)
                }
            };
            if slot.replace(value).is_some() {
                return Err(redirections_error(format_args!(
                    "references[{ordinal}].properties repeats {name}"
                )));
            }
        }
        let neutron_role = role.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronRole"
            ))
        })?;
        let neutron_role = required_text(
            neutron_role,
            format_args!("references[{ordinal}].properties.neutronRole.value"),
        )?;
        let neutron_data = data.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronData"
            ))
        })?;
        let id = format!("f3d:xref:reference#{ordinal}");
        Ok(XrefReference {
            current_pose: None,
            operation_pose: None,
            contained_poses: Vec::new(),
            owned_poses: Vec::new(),
            id,
            ordinal: ordinal_at(ordinal)?,
            occurrence_ordinal: 0,
            placement_source: None,
            from,
            relative_path,
            neutron_role,
            neutron_data,
            transform: None,
        })
    }
}

fn redirections_error(message: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("{REDIRECTIONS_ENTRY}: {message}"))
}

fn required_text(
    value: String,
    field: impl std::fmt::Display,
) -> Result<crate::records::xref::RequiredXrefText, CodecError> {
    value
        .try_into()
        .map_err(|_| redirections_error(format_args!("{field} must be non-empty")))
}

/// Validate `ComponentReferenceData.json`, if present.
///
/// The stable grammar is an open top-level JSON object. Member names and values
/// are application-defined extension data: the codec validates the envelope,
/// preserves the original ZIP entry byte-for-byte, and performs no semantic
/// projection without a separately identified field contract.
fn validate_component_reference_data(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    if !scan
        .entries
        .iter()
        .any(|entry| entry.name == COMPONENT_REFERENCE_ENTRY)
    {
        return Ok(());
    }
    parse_component_reference_data(ctx, scan.entry_bytes(COMPONENT_REFERENCE_ENTRY)?)?;
    Ok(())
}

fn parse_component_reference_data(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<serde_json::Value, CodecError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let value = ctx
        .parse_json_value(text, "parse F3D component reference JSON")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    if !value.is_object() {
        return Err(CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} must contain a top-level JSON object"
        )));
    }
    Ok(value)
}

fn reference_data(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<XrefTable>, CodecError> {
    validate_component_reference_data(ctx, scan)?;
    let bytes = match scan.entry_bytes(REDIRECTIONS_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    parse(ctx, bytes).map(Some)
}

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<XrefTable>, CodecError> {
    decode_with_scopes(ctx, scan, &[])
}

/// Parse the external-reference table and bind its occurrences to exact
/// `Component Insert` constructions already decoded from the Design streams.
pub(crate) fn decode_with_scopes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Option<XrefTable>, CodecError> {
    let Some(mut table) = reference_data(ctx, scan)? else {
        return Ok(None);
    };
    bind_occurrences(scan, &mut table, scopes)?;
    Ok(Some(table))
}

/// The `u32` ordinal for an enumerated position, or a malformed-input error.
fn ordinal_at(position: usize) -> Result<u32, CodecError> {
    u32::try_from(position).map_err(|_| {
        CodecError::malformed(format_args!(
            "F3D external-reference ordinal {position} exceeds u32"
        ))
    })
}

/// Parse `RedirectionsStream.dat` bytes into an [`XrefTable`].
fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<XrefTable, CodecError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let parsed: RedirectionsJson = ctx
        .parse_json(text, "parse F3D redirections JSON")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    if parsed.name != "RedirectionsStream" {
        return Err(redirections_error(format_args!(
            "name must be RedirectionsStream"
        )));
    }
    if parsed.schema_version != 0 {
        let message = format!(
            "F3D {REDIRECTIONS_ENTRY}: unsupported schema-version {}",
            parsed.schema_version
        );
        return Err(CodecError::NotImplemented(message));
    }
    if parsed.designs.is_empty() {
        return Err(redirections_error(
            "designs must contain the document entry",
        ));
    }
    let mut designs = Vec::new();
    for (ordinal, design) in parsed.designs.into_iter().enumerate() {
        let target_file_name = required_text(
            design.target_file_name,
            format_args!("designs[{ordinal}].targetFileName"),
        )?;
        let id = format!("f3d:xref:design#{ordinal}");

        designs.push(XrefDesign {
            id,
            ordinal: ordinal_at(ordinal)?,
            file_version: design.file_version,
            target_file_name,
            display_name: design.display_name,
            lineage_urn: design.lineage_urn,
            version_urn: design.version_urn,
        });
    }
    let references = match parsed.references {
        ReferencesJson::List(references) if !references.is_empty() => references,
        ReferencesJson::List(_) => {
            return Err(redirections_error(
                "references must be {} when the document has no outgoing references",
            ));
        }
        ReferencesJson::Leaf {} => Vec::new(),
    };
    let mut admitted_references = Vec::new();
    let mut derived_references = Vec::new();
    for (ordinal, reference) in references.into_iter().enumerate() {
        match reference {
            ReferenceJson::Xref(fields) => admitted_references.push(fields.into_record(ordinal)?),
            ReferenceJson::Derived(fields) => {
                let reference = fields.into_record(ordinal)?;
                derived_references.push(DerivedReference {
                    id: format!("f3d:derived:reference#{ordinal}"),
                    ordinal: reference.ordinal,
                    from: reference.from,
                    relative_path: reference.relative_path,
                    neutron_role: reference.neutron_role,
                    neutron_data: reference.neutron_data,
                });
            }
        }
    }
    Ok(XrefTable {
        designs,
        references: admitted_references,
        derived_references,
        ..Default::default()
    })
}

/// Parse the `docstruct` declaration of a non-empty `Properties.dat`, if
/// present. The entry is a `u32` payload byte count followed by that many
/// JSON bytes; count 0 is the empty slot and carries no declaration.
pub(crate) fn docstruct(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<Docstruct>, CodecError> {
    let bytes = match scan.entry_bytes(PROPERTIES_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut view = View::over_retained(bytes);
    let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    let Some(payload) = view.take(count) else {
        return Ok(None);
    };

    let Ok(text) = std::str::from_utf8(payload) else {
        return Ok(None);
    };
    let value = match ctx.parse_json_value(text, "parse F3D properties JSON") {
        Ok(value) => value,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let Some(docstruct) = value.get("docstruct") else {
        return Ok(None);
    };
    let Some(doc_type) = docstruct.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let doc_type = doc_type.to_owned();
    let subtype = docstruct
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .map(std::borrow::ToOwned::to_owned);
    Ok(Some(Docstruct { doc_type, subtype }))
}

/// An outgoing XREF document with explicitly empty model-body maps is an
/// assembly. Without that map state, require the document-type declaration
/// and absence of BREP streams.
pub(crate) fn is_assembly(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: Option<&XrefTable>,
    model_body_maps_empty: bool,
) -> Result<bool, CodecError> {
    if table.is_none_or(|table| table.references.is_empty()) {
        return Ok(false);
    }
    if model_body_maps_empty {
        return Ok(true);
    }
    if crate::container::design_breps(scan).next().is_some() {
        return Ok(false);
    }
    Ok(docstruct(ctx, scan)?.is_some_and(|docstruct| docstruct.doc_type == "assembly-design"))
}

/// The lineage/version design entry for one reference: the entry whose
/// `target_file_name` equals the reference's `relative_path`.
pub(crate) fn design_for<'a>(
    table: &'a XrefTable,
    reference: &XrefReference,
) -> Option<&'a XrefDesign> {
    table
        .designs
        .iter()
        .find(|design| design.target_file_name == reference.relative_path)
}

/// External product structure projected into local component instance paths.
pub(crate) struct ExternalOccurrenceProjection {
    pub(crate) occurrences: Vec<Occurrence>,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Repeat external placements below each instance of their local owner.
pub(crate) fn project_occurrences(
    table: &XrefTable,
    sources: &crate::design::components::LocalComponentSources<'_>,
    local: &[Occurrence],
) -> Result<ExternalOccurrenceProjection, CodecError> {
    use crate::loss::F3dLossCode;
    use std::collections::BTreeMap;

    let carriers = sources
        .occurrences
        .iter()
        .map(|carrier| (carrier.id.as_str(), carrier))
        .collect::<BTreeMap<_, _>>();
    let mut instances = BTreeMap::<(&str, u64), Vec<_>>::new();
    let mut ordinals = BTreeMap::new();
    for occurrence in local {
        if let OccurrenceParent::Occurrence { occurrence: parent } = &occurrence.parent {
            let next = u64::from(occurrence.ordinal) + 1;
            let ordinal = ordinals.entry(parent).or_insert(0);
            *ordinal = (*ordinal).max(next);
        }
        let Some(carrier) = occurrence
            .native_ref
            .as_deref()
            .and_then(|id| carriers.get(id))
        else {
            continue;
        };
        let Some(stream) = crate::ids::native_stream(&carrier.id) else {
            continue;
        };
        let Some(definition) = sources
            .components
            .get(stream)
            .and_then(|components| components.roots.get(&carrier.component_record_index))
        else {
            continue;
        };
        instances
            .entry((stream, definition.root_entity))
            .or_default()
            .push(&occurrence.id);
    }
    let mut projection = ExternalOccurrenceProjection {
        occurrences: Vec::new(),
        losses: Vec::new(),
    };
    // An occurrence at the document root takes its current pose: the world
    // transform its component-path carrier stores.
    let mut current_poses = 0usize;
    let mut native_poses = 0usize;
    let at_root = |mut occurrence: Occurrence,
                   reference: &XrefReference,
                   current_poses: &mut usize|
     -> Result<Occurrence, CodecError> {
        if let Some(pose) = &reference.current_pose {
            occurrence.transform =
                crate::design::components::neutral_transform(pose.transform.map_or(
                    IDENTITY_ROWS,
                    crate::records::xref::XrefPlacementTransform::rows,
                ))?;
            *current_poses += 1;
        }
        Ok(occurrence)
    };
    for (reference, occurrence) in table.references.iter().zip(root_occurrences(table)?) {
        if !has_decoded_placement(reference) {
            // Mixed tables have no earlier placement to keep: main refused the complete table.
            if !table.derived_references.is_empty() {
                projection.losses.push(F3dLossCode::XrefOccurrenceUnplaced.note(format!(
                    "xref {}: member {} stays native because its occurrence placement did not decode; its occurrence and member subtree are omitted",
                    reference.id, reference.relative_path.as_str()
                )));
                continue;
            }
            projection.losses.push(F3dLossCode::XrefPlacementIdentityAssumed.note(format!(
                "xref {}: member {} occurrence placement did not decode; it was placed at the identity without proof",
                reference.id, reference.relative_path.as_str()
            )));
        }
        let owner = reference
            .placement_source
            .as_ref()
            .and_then(|source| source.owner.as_ref().map(|owner| (source, owner)));
        let Some((source, owner)) = owner else {
            projection
                .occurrences
                .push(at_root(occurrence, reference, &mut current_poses)?);
            continue;
        };
        let components = sources.components.get(&source.stream);
        let root = components.and_then(|components| {
            components
                .root_component
                .and_then(|record| components.roots.get(&record))
        });
        if root.is_some_and(|root| root.root_entity == owner.target) {
            projection
                .occurrences
                .push(at_root(occurrence, reference, &mut current_poses)?);
            continue;
        }
        let unique = components.is_some_and(|components| {
            components
                .roots
                .values()
                .filter(|definition| definition.root_entity == owner.target)
                .count()
                == 1
        });
        let parents = instances
            .get(&(source.stream.as_str(), owner.target))
            .filter(|_| unique);
        let Some(parents) = parents else {
            if reference.current_pose.is_some() {
                projection.losses.push(F3dLossCode::XrefOwnerUnresolved.note(format!(
                    "external occurrence carrier {} in {}: local owner {} has no unique instantiated component; the decoded world carrier places the occurrence at the document root",
                    source.carrier_record_index, source.stream, owner.target
                )));
                projection
                    .occurrences
                    .push(at_root(occurrence, reference, &mut current_poses)?);
            } else if let Some(owner_world) =
                crate::design::components::owners::placement(sources, &source.stream, owner.target)
            {
                let mut occurrence = occurrence;
                occurrence.transform =
                    owner_world.compose(occurrence.transform).map_err(|error| {
                        CodecError::malformed(format!(
                            "external owner placement composition: {error}"
                        ))
                    })?;
                projection.occurrences.push(occurrence);
            } else {
                projection.losses.push(F3dLossCode::XrefOwnerUnresolved.note(format!(
                    "external occurrence carrier {} in {}: local owner {} has no unique instantiated component; placement remains at the document root",
                    source.carrier_record_index, source.stream, owner.target
                )));
                projection.occurrences.push(occurrence);
            }
            continue;
        };
        if reference.current_pose.is_some() {
            native_poses += 1;
        }
        for parent in parents {
            let used = cadmpeg_core::decode::u64_from_index(local.len())
                + cadmpeg_core::decode::u64_from_index(projection.occurrences.len());
            if used >= sources.limits.max_entities {
                return Err(cadmpeg_core::decode::ResourceLimit {
                    dimension: cadmpeg_core::decode::ResourceDimension::Entities,
                    reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
                    limit: sources.limits.max_entities,
                    used,
                    additional: 1,
                    operation: "expand F3D external occurrences",
                }
                .into());
            }
            let mut child = occurrence.clone();
            child.id = crate::ids::nested_xref_occurrence_id(
                parent,
                reference.ordinal,
                reference.occurrence_ordinal,
            );
            child.parent = OccurrenceParent::Occurrence {
                occurrence: (*parent).clone(),
            };
            let ordinal = ordinals.entry(parent).or_insert(0);
            child.ordinal = u32::try_from(*ordinal).map_err(|_| {
                CodecError::malformed("external occurrence sibling ordinal exceeds u32::MAX")
            })?;
            *ordinal += 1;
            projection.occurrences.push(child);
        }
    }
    if current_poses != 0 {
        projection.losses.push(F3dLossCode::XrefPlacementCurrentPose.note(format!(
            "{current_poses} external occurrence(s) at the document root take their current pose from a component-path carrier; their occurrence-placement matrices stay in the native frames"
        )));
    }
    if native_poses != 0 {
        projection.losses.push(F3dLossCode::XrefCurrentPoseNative.note(format!(
            "{native_poses} external placement(s) below a local owner keep their occurrence-placement matrix in every projected instance; the world pose of their component-path carrier stays native"
        )));
    }
    Ok(projection)
}

fn has_decoded_placement(reference: &XrefReference) -> bool {
    reference.placement_source.is_some()
        || reference.transform.is_some()
        || reference.current_pose.is_some()
}

pub(crate) fn local_root_ordinal_start(table: &XrefTable) -> usize {
    if table.derived_references.is_empty() {
        return table.references.len();
    }
    table
        .references
        .iter()
        .rposition(has_decoded_placement)
        .map_or(0, |ordinal| ordinal + 1)
}

pub(crate) fn has_projected_placement(
    reference: &XrefReference,
    occurrences: &[Occurrence],
) -> bool {
    occurrences
        .iter()
        .any(|occurrence| occurrence.native_ref.as_deref() == Some(reference.id.as_str()))
}

pub(crate) const IDENTITY_ROWS: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Construct document-frame placements before local owner expansion.
fn root_occurrences(table: &XrefTable) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
    let mut occurrences = Vec::new();
    for (ordinal, reference) in table.references.iter().enumerate() {
        let path = reference.relative_path.as_str().to_owned();
        let transform = reference.transform.map_or(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            crate::records::xref::XrefPlacementTransform::rows,
        );
        occurrences.push(Occurrence {
            id: crate::ids::neutral_xref_occurrence_id(
                reference.ordinal,
                reference.occurrence_ordinal,
            ),
            prototype: PrototypeReference::External {
                document: ExternalDocument::path(path),
                object: None,
            },
            parent: OccurrenceParent::Root {},
            ordinal: ordinal_at(ordinal)?,
            transform: crate::design::components::neutral_transform(transform)?,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(reference.id.as_str().to_owned()),
        });
    }
    Ok(occurrences)
}

/// Return the occurrence bound to a construction carrier in its Design stream.
pub(crate) fn component_insert_reference<'a>(
    references: &'a [XrefReference],
    stream: &str,
    construction: &DesignComponentInsertConstruction,
) -> Option<&'a XrefReference> {
    let mut matches = references.iter().filter(|reference| {
        reference.neutron_role.as_str() == construction.neutron_role
            && reference.placement_source.as_ref().is_some_and(|source| {
                source.stream == stream
                    && source.carrier_record_index == u64::from(construction.carrier_record_index)
            })
            && reference
                .transform
                .map(crate::records::xref::XrefPlacementTransform::rows)
                == Some((*construction.transform()).into())
    });
    let reference = matches.next()?;
    matches.next().is_none().then_some(reference)
}

/// Return the unique projected instance of one native external placement.
pub(crate) fn unique_occurrence<'a>(
    reference: &XrefReference,
    occurrences: &'a [Occurrence],
) -> Option<&'a Occurrence> {
    let mut matches = occurrences.iter().filter(|occurrence| {
        matches!(occurrence.prototype, PrototypeReference::External { .. })
            && occurrence.native_ref.as_deref() == Some(reference.id.as_str())
    });
    let occurrence = matches.next()?;
    matches.next().is_none().then_some(occurrence)
}

/// Resolve exact `Component Insert` history scopes to their placed occurrences.
pub(crate) fn bind_component_insert_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    table: &XrefTable,
    occurrences: &[Occurrence],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for scope in scopes {
        let Some(construction) = scope.component_insert_construction() else {
            continue;
        };
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(reference) = component_insert_reference(&table.references, stream, construction)
        else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            let Some(occurrence) = unique_occurrence(reference, occurrences) else {
                losses.push(
                    crate::loss::F3dLossCode::FeatureOccurrencePathUnresolved.note(
                        "an external component feature has no unique expanded occurrence path",
                    ),
                );
                continue;
            };
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::InsertComponent {
                        occurrence: occurrence.id.clone(),
                    },
                ));
        }
    }
    losses
}

/// Expand container references through their occurrence records in the active
/// Design `BulkStream` and retain each occurrence-local placement matrix.
fn bind_occurrences(
    scan: &ContainerScan,
    table: &mut XrefTable,
    scopes: &[DesignParameterScope],
) -> Result<(), CodecError> {
    let mut streams = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_entry = entry
            .name
            .strip_suffix("BulkStream.dat")
            .and_then(|prefix| {
                scan.entries
                    .iter()
                    .find(|candidate| candidate.name.strip_prefix(prefix) == Some("MetaStream.dat"))
            });
        let (serializer_magic, headers, carriers) = if let Some(meta_entry) = meta_entry {
            let meta = scan.parsed_metastream(&meta_entry.name)?;
            let meta_bytes = scan.entry_bytes(&meta_entry.name)?;
            (
                Some(crate::metastream::serializer_magic(
                    meta_bytes,
                    &meta_entry.name,
                )?),
                typed_occurrence_placement_records(bytes, &meta)?,
                typed_records(bytes, &meta, "CommonData", COMPONENT_PATH_TYPE_GUID)?,
            )
        } else {
            table.placement_metadata_missing = true;
            (None, Vec::new(), Vec::new())
        };
        let (placements, failures) =
            occurrence_placements_with_failures(bytes, &headers, serializer_magic, None)?;
        let poses = carriers
            .iter()
            .filter_map(|record| bytes.get(record.offset..record.end))
            .filter_map(|body| component_path_pose(body, serializer_magic))
            .collect::<Vec<_>>();
        let contained = carriers
            .iter()
            .filter_map(|record| bytes.get(record.offset..record.end))
            .filter_map(|body| contained_component_path_pose(body, serializer_magic))
            .collect::<Vec<_>>();
        let owned = carriers
            .iter()
            .filter_map(|record| bytes.get(record.offset..record.end))
            .filter_map(|body| owned_component_path_pose(body, serializer_magic))
            .collect::<Vec<_>>();

        streams.push((
            placements,
            failures,
            crate::ids::native_scope(&entry.name),
            (poses, contained, owned),
        ));
    }
    let mut expanded = Vec::new();
    let placement_failures = streams
        .iter()
        .flat_map(|(_, failures, _, _)| failures.iter().cloned())
        .collect();
    let unbound_legacy_placements = streams
        .iter()
        .flat_map(|(placements, _, _, _)| placements)
        .filter(|placement| {
            legacy_attributed_placement_details(&placement.frame_bytes).is_some()
                && !table.references.iter().any(|reference| {
                    placement
                        .link_names
                        .iter()
                        .any(|name| name == reference.neutron_role.as_str())
                })
        })
        .map(|placement| OccurrencePlacementFailure {
            carrier_record_index: Some(placement.carrier_record_index),
            link_names: placement.link_names.clone(),
        })
        .collect();
    let mut placement_overrides = Vec::new();
    let mut ambiguous_current_poses = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (placements, _, stream, (poses, contained, owned)) in &streams {
            let direct = select_component_insert_transforms(
                scopes.iter().filter_map(|scope| {
                    let stream = crate::ids::native_stream(&scope.id)?;
                    let construction = scope.component_insert_construction()?;
                    Some((stream, construction))
                }),
                stream,
                &reference.neutron_role,
            );
            let structured_count =
                superseded_placement_count(&direct, placements, &reference.neutron_role);
            if !direct.is_empty() && structured_count != 0 {
                placement_overrides.push(PlacementOverride {
                    ordinal: reference.ordinal,
                    count: structured_count,
                });
            }
            let selected =
                occurrence_transforms_with_precedence(direct, placements, &reference.neutron_role);

            for placement in selected {
                let pose = match current_pose(poses, &placement, &reference.neutron_role) {
                    CurrentPose::One(pose) => Some(pose),
                    CurrentPose::None => None,
                    CurrentPose::Ambiguous(carriers) => {
                        ambiguous_current_poses.push((placement.carrier_record_index, carriers));
                        None
                    }
                };
                let contained = contained
                    .iter()
                    .filter(|pose| {
                        pose.placement_record_index == placement.carrier_record_index
                            && pose.role == reference.neutron_role.as_str()
                    })
                    .filter_map(|pose| {
                        Some(crate::records::xref::XrefContainedPose {
                            carrier_record_index: pose.carrier_record_index,
                            frame_bytes: pose.frame_bytes.clone().into(),
                            contained_record_index: pose.contained_record_index,
                            owner_record_indices: pose.owner_record_indices.clone(),
                            transform: pose
                                .transform
                                .map(crate::records::xref::XrefPlacementTransform::try_from)
                                .transpose()
                                .ok()?,
                        })
                    })
                    .collect::<Vec<_>>();
                let owned = owned
                    .iter()
                    .filter(|pose| {
                        pose.placement_record_index == placement.carrier_record_index
                            && pose.role == reference.neutron_role.as_str()
                    })
                    .filter_map(|pose| {
                        Some(crate::records::xref::XrefOwnedPose {
                            carrier_record_index: pose.carrier_record_index,
                            frame_bytes: pose.frame_bytes.clone().into(),
                            owner_record_indices: pose.owner_record_indices.clone(),
                            transform: pose
                                .transform
                                .map(crate::records::xref::XrefPlacementTransform::try_from)
                                .transpose()
                                .ok()?,
                        })
                    })
                    .collect::<Vec<_>>();
                occurrences.push((placement, stream.clone(), pose, (contained, owned)));
            }
        }
        if occurrences.is_empty() {
            expanded.push(reference.clone());
            continue;
        }
        let keep_reference_id = table.derived_references.is_empty()
            && occurrences.len() == 1
            && legacy_attributed_placement_details(&occurrences[0].0.frame_bytes).is_some()
            && View::u16_le_at(
                &occurrences[0].0.frame_bytes,
                legacy_prefix::LEN + legacy_attributes::LEN + legacy_members::CONTROL,
            ) == Some(legacy_identity_control::CONTROL_VALUE);
        for (
            occurrence_ordinal,
            (placement, stream, current_pose, (contained_poses, owned_poses)),
        ) in occurrences.into_iter().enumerate()
        {
            let occurrence_id = if keep_reference_id {
                reference.id.clone()
            } else {
                format!(
                    "f3d:xref:reference#{}-occurrence-{occurrence_ordinal}",
                    reference.ordinal
                )
            };
            expanded.push(XrefReference {
                id: occurrence_id,
                occurrence_ordinal: ordinal_at(occurrence_ordinal)?,
                placement_source: Some(XrefPlacementSource {
                    frame_bytes: placement.frame_bytes.into(),
                    stream,
                    occurrence_guid: placement.occurrence_guid,
                    carrier_record_index: placement.carrier_record_index,
                    owner: placement.owner,
                }),
                transform: placement
                    .transform
                    .map(crate::records::xref::XrefPlacementTransform::try_from)
                    .transpose()
                    .map_err(CodecError::NotImplemented)?,
                current_pose,
                contained_poses,
                owned_poses,
                ..reference.clone()
            });
        }
    }
    table.references = expanded;
    table.placement_failures = placement_failures;
    table.unbound_legacy_placements = unbound_legacy_placements;
    table.placement_overrides = placement_overrides;
    table.ambiguous_current_poses = ambiguous_current_poses;
    Ok(())
}

/// The component-path carriers that name one placement.
enum CurrentPose {
    None,
    One(crate::records::xref::XrefCurrentPose),
    /// The carrier records of two or more matching carriers.
    Ambiguous(Vec<u64>),
}

/// The current pose of one placement: the unique component-path carrier in the
/// same Design stream whose path names this placement record and role.
fn current_pose(
    poses: &[ComponentPathPose],
    placement: &OccurrencePlacement,
    role: &str,
) -> CurrentPose {
    let matching = poses
        .iter()
        .filter(|pose| {
            pose.placement_record_index == placement.carrier_record_index && pose.role == role
        })
        .collect::<Vec<_>>();
    let pose = match matching.as_slice() {
        [] => return CurrentPose::None,
        [pose] => pose,
        many => {
            return CurrentPose::Ambiguous(
                many.iter().map(|pose| pose.carrier_record_index).collect(),
            )
        }
    };
    let Ok(transform) = pose
        .transform
        .map(crate::records::xref::XrefPlacementTransform::try_from)
        .transpose()
    else {
        return CurrentPose::None;
    };
    CurrentPose::One(crate::records::xref::XrefCurrentPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: pose.frame_bytes.clone().into(),
        transform,
    })
}

/// Select the exact `Component Insert` constructions for one Design stream
/// and external-reference role. The construction parser has already joined
/// each role to its scope-owned relation record and verified its carrier
/// transform, so the class tag is not an admission discriminator here.
fn select_component_insert_transforms<'a, I>(
    constructions: I,
    stream: &str,
    role: &str,
) -> Vec<(u64, [[f64; 4]; 4])>
where
    I: IntoIterator<Item = (&'a str, &'a DesignComponentInsertConstruction)>,
{
    constructions
        .into_iter()
        .filter(|(construction_stream, construction)| {
            *construction_stream == stream && construction.neutron_role == role
        })
        .map(|(_, construction)| {
            (
                u64::from(construction.carrier_record_index),
                construction.transform().rows(),
            )
        })
        .collect::<Vec<_>>()
}

/// Apply a scope transform once to its carrier and keep the other placements.
fn occurrence_transforms_with_precedence(
    direct: Vec<(u64, [[f64; 4]; 4])>,
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<OccurrencePlacement> {
    let mut selected = Vec::new();
    let mut overridden = HashSet::new();
    for placement in placements
        .iter()
        .filter(|placement| placement.link_names.iter().any(|name| name == role))
    {
        let mut placement = placement.clone();
        if let Some((carrier, transform)) = direct
            .iter()
            .find(|(carrier, _)| *carrier == placement.carrier_record_index)
        {
            if !overridden.insert(*carrier) {
                continue;
            }
            placement.transform = Some(*transform);
        }
        selected.push(placement);
    }
    for (carrier_record_index, transform) in direct {
        if overridden.insert(carrier_record_index) {
            selected.push(OccurrencePlacement {
                frame_bytes: Vec::new(),
                occurrence_guid: None,
                carrier_record_index,
                owner: None,
                link_names: vec![role.to_owned()],
                transform: Some(transform),
            });
        }
    }
    selected
}

/// Count placements whose carrier also has a scope-bound transform.
fn superseded_placement_count(
    direct: &[(u64, [[f64; 4]; 4])],
    placements: &[OccurrencePlacement],
    role: &str,
) -> usize {
    placements
        .iter()
        .filter(|placement| {
            placement.link_names.iter().any(|name| name == role)
                && direct
                    .iter()
                    .any(|(carrier, _)| *carrier == placement.carrier_record_index)
        })
        .count()
}

/// Return the `BulkStream` offsets whose type-table identity is the stable
/// occurrence-placement type. Dynamic class tags and record shape are not
/// sufficient because unrelated component records can share that shape.
fn typed_occurrence_placement_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<IndexedRecord>, CodecError> {
    typed_records(bytes, meta, "Component", OCCURRENCE_PLACEMENT_TYPE_GUID)
}

/// Return the `BulkStream` record extents registered to one type-table
/// identity, checking each primary header against its registration.
fn typed_records(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    module: &str,
    type_guid: &str,
) -> Result<Vec<IndexedRecord>, CodecError> {
    let mut placement_entities = std::collections::HashMap::new();
    for (ordinal, design_type) in meta.types.iter().enumerate().filter(|(_, design_type)| {
        design_type.module == module
            && design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(type_guid)
    }) {
        for &entity in design_type.entities.values() {
            placement_entities.insert(entity, ordinal);
        }
    }
    let mut records = Vec::new();
    for frame in crate::metastream::primary_record_frames(meta, bytes.len())? {
        let Some(&ordinal) = placement_entities.get(&frame.entity_id) else {
            continue;
        };
        let header = bytes.get(frame.start..frame.member_end).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D {module} record index is outside its BulkStream"
            ))
        })?;
        let class_matches = lp_ascii_strict(header, 0, 3..=3)
            .and_then(|(tag, _)| tag.parse::<usize>().ok())
            .and_then(|tag| tag.checked_sub(256))
            == Some(ordinal);
        if !class_matches || View::u64_le_at(header, 7) != Some(frame.entity_id) {
            return Err(CodecError::malformed(format_args!(
                "F3D {module} record header disagrees with its registered primary index"
            )));
        }
        records.push(IndexedRecord {
            offset: frame.start,
            end: frame.member_end,
        });
    }
    Ok(records)
}

#[derive(Debug)]
struct IndexedRecord {
    offset: usize,
    end: usize,
}

/// The transforms of every placement whose target path carries `role`, in
/// record order. One occurrence-placement record is one occurrence.
#[cfg(test)]
fn occurrence_transforms(
    placements: &[OccurrencePlacement],
    role: &str,
) -> Vec<Option<[[f64; 4]; 4]>> {
    placements
        .iter()
        .filter(|placement| placement.link_names.iter().any(|name| name == role))
        .map(|placement| placement.transform)
        .collect::<Vec<_>>()
}

#[cfg(test)]
fn indexed_records(bytes: &[u8]) -> Vec<IndexedRecord> {
    let mut records: Vec<IndexedRecord> = Vec::new();
    for at in bytes
        .len()
        .checked_sub(11)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if View::u32_le_at(bytes, at) != Some(3) {
            continue;
        }
        let Some(tag) = bytes.get(at + 4..at + 7) else {
            continue;
        };
        if !tag.iter().all(u8::is_ascii_digit) || bytes.get(at + 7..at + 15).is_none() {
            continue;
        }
        if let Some(previous) = records.last_mut() {
            previous.end = at;
        }

        records.push(IndexedRecord {
            offset: at,
            end: bytes.len(),
        });
    }
    records
}

/// One occurrence-placement record: the target path it names and the transform
/// it places that path at
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)
/// "**Placement.**").
#[derive(Debug, Clone, PartialEq)]
struct OccurrencePlacement {
    frame_bytes: Vec<u8>,
    occurrence_guid: Option<crate::records::mesh::DesignRelaxedGuidText>,
    carrier_record_index: u64,
    owner: Option<XrefLocalOwner>,
    /// Cross-document link names carried by the path elements, in path order.
    /// The role-bearing element is not necessarily the first.
    link_names: Vec<String>,
    /// `None` is the stored identity form, which carries no matrix.
    transform: Option<[[f64; 4]; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OccurrencePlacementFailure {
    pub(crate) carrier_record_index: Option<u64>,
    /// Link names recovered from the valid target-path prefix.
    pub(crate) link_names: Vec<String>,
}

/// Parse every indexed record that closes exactly under the occurrence-placement
/// grammar, in record order.
#[cfg(test)]
fn occurrence_placements(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_filtered(bytes, records, serializer_magic, None)
}

/// Parse occurrence-placement records, optionally restricted by the
/// `MetaStream` type-table admission set.
#[cfg(test)]
fn occurrence_placements_filtered(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_with_failures(bytes, records, serializer_magic, typed_offsets)
        .expect("test placement parse")
        .0
}

/// Parse admitted placement records and retain role names from records whose
/// target path is valid but whose remaining generation-specific payload is not.
fn occurrence_placements_with_failures(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Result<(Vec<OccurrencePlacement>, Vec<OccurrencePlacementFailure>), CodecError> {
    let mut placements = Vec::new();
    let mut failures = Vec::new();
    for record in records
        .iter()
        .filter(|record| typed_offsets.is_none_or(|offsets| offsets.contains(&record.offset)))
    {
        let Some(body) = bytes.get(record.offset..record.end) else {
            continue;
        };
        if lp_ascii_strict(body, root_instance::INSTANCE_NAME_LENGTH, 0..=256)
            .is_some_and(|(name, _)| name.as_bytes() == ROOT_INSTANCE_NAME)
            || occurrence_path_elements(body).is_some_and(|(names, _, _)| names.is_empty())
        {
            continue;
        }
        if let Some(placement) = occurrence_placement(body, serializer_magic)? {
            placements.push(placement);
        } else {
            let link_names = occurrence_path(body)
                .map(|(names, _)| names)
                .or_else(|| legacy_occurrence_role(body).map(|role| vec![role.to_string()]))
                .unwrap_or_default();
            failures.push(OccurrencePlacementFailure {
                carrier_record_index: View::u64_le_at(body, 7),
                link_names,
            });
        }
    }
    Ok((placements, failures))
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Result<Option<OccurrencePlacement>, CodecError> {
    let Some(carrier_record_index) = View::u64_le_at(body, 7) else {
        return Ok(None);
    };
    if let Some(details) = legacy_attributed_placement_details(body) {
        return Ok(Some(OccurrencePlacement {
            frame_bytes: body.to_vec(),
            occurrence_guid: Some(details.occurrence_guid),
            carrier_record_index: details.carrier_record_index,
            owner: Some(XrefLocalOwner {
                target: details.owner,
                type_guid: None,
            }),
            link_names: vec![details.role.to_string()],
            transform: details.transform,
        }));
    }
    if let Some((role, transform)) = legacy_occurrence_placement(body) {
        let role = role.to_string();
        let link_names = [role].into_iter().collect::<Vec<_>>();
        return Ok(Some(OccurrencePlacement {
            frame_bytes: Vec::new(),
            occurrence_guid: None,
            carrier_record_index,
            owner: None,
            link_names,
            transform,
        }));
    }
    if let Some(placement) = repeated_target_occurrence_placement(body)? {
        return Ok(Some(placement));
    }
    if let Some(placement) = modern_occurrence_placement(body, serializer_magic) {
        return Ok(Some(placement));
    }
    let Some(record_index) = View::u32_le_at(body, 7) else {
        return Ok(None);
    };
    let Some((link_name, _)) = grouped_component_insert_identity(body, 0, body.len(), record_index)
    else {
        return Ok(None);
    };
    let link_names = vec![link_name.to_string()];
    Ok(Some(OccurrencePlacement {
        frame_bytes: Vec::new(),
        occurrence_guid: None,
        carrier_record_index: u64::from(record_index),
        owner: None,
        link_names,
        transform: None,
    }))
}

/// Parse the placement generation that repeats the target identity after the
/// standard path and stores the identity flag beside that repeated target.
fn repeated_target_occurrence_placement(
    body: &[u8],
) -> Result<Option<OccurrencePlacement>, CodecError> {
    Ok(
        repeated_target_occurrence_placement_details(body)?.map(|details| OccurrencePlacement {
            frame_bytes: body.to_vec(),
            carrier_record_index: details.carrier_record_index,
            occurrence_guid: Some(details.occurrence_guid),
            owner: Some(details.owner),
            link_names: details.link_names,
            transform: details.transform.map(|(_, matrix)| matrix),
        }),
    )
}

struct RepeatedTargetPlacementDetails {
    occurrence_guid: crate::records::mesh::DesignRelaxedGuidText,
    carrier_record_index: u64,
    owner: XrefLocalOwner,
    link_names: Vec<String>,
    role: String,
    role_offset: usize,
    transform: Option<(usize, [[f64; 4]; 4])>,
}

pub(crate) fn placement_source_matches(reference: &XrefReference) -> Result<bool, CodecError> {
    let Some(source) = &reference.placement_source else {
        return Ok(true);
    };
    if let Some(details) = legacy_attributed_placement_details(&source.frame_bytes) {
        return Ok(
            source.occurrence_guid.as_ref() == Some(&details.occurrence_guid)
                && details.role.eq_str(reference.neutron_role.as_str())
                && reference.transform.map_or_else(
                    || cadmpeg_ir::transform::Transform::identity().rows(),
                    crate::records::xref::XrefPlacementTransform::rows,
                ) == details
                    .transform
                    .unwrap_or_else(|| cadmpeg_ir::transform::Transform::identity().rows())
                && source.carrier_record_index == details.carrier_record_index
                && source.owner.as_ref().is_some_and(|owner| {
                    owner.target == details.owner && owner.type_guid.is_none()
                }),
        );
    }
    let Some(guid) = source.occurrence_guid.as_ref() else {
        return Ok(source.frame_bytes.is_empty());
    };
    Ok(
        repeated_target_occurrence_placement_details(&source.frame_bytes)?.is_some_and(|details| {
            details.occurrence_guid == *guid
                && details.carrier_record_index == source.carrier_record_index
                && source.owner.as_ref() == Some(&details.owner)
        }),
    )
}

/// Re-read a retained current pose through the carrier parser and require it
/// to name this reference's role and placement record.
pub(crate) fn current_pose_matches(reference: &XrefReference) -> bool {
    let Some(current) = &reference.current_pose else {
        return true;
    };
    let Some(source) = &reference.placement_source else {
        return false;
    };
    [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
        .into_iter()
        .filter_map(|magic| component_path_pose(&current.frame_bytes, magic))
        .any(|pose| {
            pose.carrier_record_index == current.carrier_record_index
                && pose.placement_record_index == source.carrier_record_index
                && pose.role == reference.neutron_role.as_str()
                && pose.transform
                    == current
                        .transform
                        .map(crate::records::xref::XrefPlacementTransform::rows)
        })
}

pub(crate) fn legacy_placement_metadata_native(source: &XrefPlacementSource) -> bool {
    legacy_attributed_placement_details(&source.frame_bytes).is_some()
}

struct LegacyAttributedPlacementDetails<'a> {
    carrier_record_index: u64,
    occurrence_guid: crate::records::mesh::DesignRelaxedGuidText,
    owner: u64,
    role: Utf16View<'a>,
    transform: Option<[[f64; 4]; 4]>,
}

fn legacy_attributed_placement_details(
    body: &[u8],
) -> Option<LegacyAttributedPlacementDetails<'_>> {
    let tag_units = usize::try_from(legacy_prefix::CLASS_TAG_LENGTH_VALUE).ok()?;
    let (tag, _) = lp_ascii_strict(body, legacy_prefix::CLASS_TAG_LENGTH, tag_units..=tag_units)?;
    if !tag.as_bytes().iter().all(u8::is_ascii_digit)
        || View::u32_le_at(body, legacy_prefix::INSTANCE_NAME_LENGTH)?
            != legacy_prefix::INSTANCE_NAME_LENGTH_VALUE
        || body.get(legacy_prefix::PATH_MARKER) != Some(&legacy_prefix::PATH_MARKER_VALUE)
        || View::u32_le_at(body, legacy_prefix::PATH_COUNT)? != legacy_prefix::PATH_COUNT_VALUE
    {
        return None;
    }
    let mut target_end = legacy_prefix::TARGET_REFERENCE;
    let first = take_legacy_occurrence_reference(body, &mut target_end)?;
    if target_end != legacy_prefix::PATH_DISCRIMINATOR
        || first.entity == 0
        || first.source_occurrence_identity == 0
        || !first
            .type_guid
            .eq_ignore_ascii_case(crate::records::feature::occurrence_transfer::COMPONENT_TYPE)
    {
        return None;
    }
    View::u32_le_at(body, legacy_prefix::PATH_DISCRIMINATOR)?;
    let attributes = body.get(legacy_prefix::LEN..)?;
    let attributes_len = legacy_placement_attributes_len(attributes)?;
    let members_start = legacy_prefix::LEN + attributes_len;
    let members = body.get(members_start..members_start + legacy_members::LEN)?;
    let control = View::u16_le_at(members, legacy_members::CONTROL)?;
    if (control != legacy_members::CONTROL_VALUE
        && !(control == legacy_identity_control::CONTROL_VALUE
            && attributes_len == legacy_attributes::LEN
            && members.get(legacy_members::IDENTITY_FLAG) == Some(&1)))
        || View::u32_le_at(members, legacy_members::ORDINAL)? == 0
        || members.get(legacy_members::REFERENCE_SEPARATOR)
            != Some(&legacy_members::REFERENCE_SEPARATOR_VALUE)
    {
        return None;
    }
    for (offset, units) in [
        (
            legacy_members::METADATA_GUID_LENGTH,
            legacy_members::METADATA_GUID_LENGTH_VALUE,
        ),
        (
            legacy_members::TRACKING_GUID_LENGTH,
            legacy_members::TRACKING_GUID_LENGTH_VALUE,
        ),
    ] {
        let units = usize::try_from(units).ok()?;
        if !lp_utf16_bounded_view(members, offset, units..=units)?
            .0
            .is_guid_relaxed()
        {
            return None;
        }
    }
    let mut reference_end = legacy_members::METADATA_REFERENCE;
    if !matches!(
        take_reference(members, &mut reference_end)?,
        Reference::CrossSegment { target, inline_type_guid: None, .. } if target != 0
    ) || reference_end != legacy_members::REFERENCE_SEPARATOR
    {
        return None;
    }
    let mut repeated_end = legacy_members::REPEATED_TARGET_REFERENCE;
    let repeated = take_legacy_occurrence_reference(members, &mut repeated_end)?;
    if repeated_end != legacy_members::IDENTITY_FLAG
        || repeated.entity != first.entity
        || repeated.source_occurrence_identity != first.source_occurrence_identity
        || !repeated.type_guid.eq_ignore_ascii_case(first.type_guid)
    {
        return None;
    }
    let tail = body.get(members_start + legacy_members::LEN..)?;
    let (tail, transform) = match *members.get(legacy_members::IDENTITY_FLAG)? {
        1 => (tail, None),
        0 if tail.len() == legacy_matrix_tail::LEN => (
            tail.get(legacy_matrix_tail::ZERO..)?,
            Some(decode_rigid_matrix(tail, legacy_matrix_tail::MATRIX)?),
        ),
        _ => return None,
    };
    if tail.len() != legacy_identity_tail::LEN
        || View::u32_le_at(tail, legacy_identity_tail::ZERO)? != legacy_identity_tail::ZERO_VALUE
        || tail.get(legacy_identity_tail::SEPARATOR) != Some(&legacy_identity_tail::SEPARATOR_VALUE)
    {
        return None;
    }
    let role_units = usize::try_from(legacy_identity_tail::ROLE_LENGTH_VALUE).ok()?;
    let (role, _) = lp_utf16_bounded_view(
        tail,
        legacy_identity_tail::ROLE_LENGTH,
        role_units..=role_units,
    )?;
    if !role.is_guid_relaxed() {
        return None;
    }
    let mut owner_end = legacy_identity_tail::OWNER_MARKER;
    let Reference::Local {
        target: owner,
        inline_type_guid: None,
    } = take_reference(tail, &mut owner_end)?
    else {
        return None;
    };
    if owner == 0 || owner_end != tail.len() {
        return None;
    }
    let tracking_units = usize::try_from(legacy_members::TRACKING_GUID_LENGTH_VALUE).ok()?;
    Some(LegacyAttributedPlacementDetails {
        carrier_record_index: View::u64_le_at(body, legacy_prefix::ENTITY)?,
        occurrence_guid: lp_utf16_bounded_view(
            members,
            legacy_members::TRACKING_GUID_LENGTH,
            tracking_units..=tracking_units,
        )?
        .0
        .to_string()
        .try_into()
        .ok()?,
        owner,
        role,
        transform,
    })
}

fn legacy_placement_attributes_len(body: &[u8]) -> Option<usize> {
    if body.get(legacy_empty_attributes::MARKER) != Some(&legacy_empty_attributes::MARKER_VALUE) {
        return None;
    }
    match View::u32_le_at(body, legacy_empty_attributes::COUNT)? {
        legacy_empty_attributes::COUNT_VALUE => Some(legacy_empty_attributes::LEN),
        legacy_attributes::COUNT_VALUE => {
            let body = body.get(..legacy_attributes::LEN)?;
            for (offset, units, expected) in [
                (
                    legacy_attributes::PARENT_NAME_LENGTH,
                    legacy_attributes::PARENT_NAME_LENGTH_VALUE,
                    legacy_attributes::PARENT_NAME_VALUE.as_slice(),
                ),
                (
                    legacy_attributes::PARENT_TYPE_LENGTH,
                    legacy_attributes::PARENT_TYPE_LENGTH_VALUE,
                    legacy_attributes::PARENT_TYPE_VALUE.as_slice(),
                ),
                (
                    legacy_attributes::RESOLUTION_NAME_LENGTH,
                    legacy_attributes::RESOLUTION_NAME_LENGTH_VALUE,
                    legacy_attributes::RESOLUTION_NAME_VALUE.as_slice(),
                ),
                (
                    legacy_attributes::RESOLUTION_TYPE_LENGTH,
                    legacy_attributes::RESOLUTION_TYPE_LENGTH_VALUE,
                    legacy_attributes::RESOLUTION_TYPE_VALUE.as_slice(),
                ),
                (
                    legacy_attributes::SELF_NAME_LENGTH,
                    legacy_attributes::SELF_NAME_LENGTH_VALUE,
                    legacy_attributes::SELF_NAME_VALUE.as_slice(),
                ),
                (
                    legacy_attributes::SELF_TYPE_LENGTH,
                    legacy_attributes::SELF_TYPE_LENGTH_VALUE,
                    legacy_attributes::SELF_TYPE_VALUE.as_slice(),
                ),
            ] {
                let units = usize::try_from(units).ok()?;
                if lp_ascii_strict(body, offset, units..=units)?.0.as_bytes() != expected {
                    return None;
                }
            }
            (body.get(legacy_attributes::PARENT_VALUE)
                == Some(&legacy_attributes::PARENT_VALUE_VALUE)
                && View::u32_le_at(body, legacy_attributes::RESOLUTION_VALUE)?
                    == legacy_attributes::RESOLUTION_VALUE_VALUE
                && body.get(legacy_attributes::SELF_VALUE)
                    == Some(&legacy_attributes::SELF_VALUE_VALUE))
            .then_some(legacy_attributes::LEN)
        }
        _ => None,
    }
}

macro_rules! xref_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn repeated_target_occurrence_placement_details(
    body: &[u8],
) -> Result<Option<RepeatedTargetPlacementDetails>, CodecError> {
    use crate::layout::pathless_external_placement_prefix as pathless;
    let (mut link_names, path_target, mut at) =
        if let Some((names, target, at)) = occurrence_target_path(body) {
            (names, Some(target), at)
        } else {
            let (tag, _) = xref_some!(lp_ascii_strict(body, 0, 3..=3));
            if !tag.as_bytes().iter().all(u8::is_ascii_digit) {
                return Ok(None);
            }
            if body
                .get(pathless::EMPTY_NAME..pathless::CONTROL)
                .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
            {
                return Ok(None);
            }
            (Vec::new(), None, pathless::CONTROL)
        };
    let control = xref_some!(View::u32_le_at(body, at));
    if control == 0 || (path_target.is_none() && control != pathless::CONTROL_VALUE) {
        return Ok(None);
    }
    at += 4;
    let (metadata_guid, next) = xref_some!(lp_utf16_bounded(body, at, 36..=36));
    if !is_guid_relaxed(&metadata_guid) {
        return Ok(None);
    }
    let (occurrence_guid, next) = xref_some!(lp_utf16_bounded(body, next, 36..=36));
    let occurrence_guid = xref_some!(occurrence_guid.try_into().ok());
    at = next;
    if body.get(at) == Some(&0) {
        at += 1;
    } else if path_target.is_none() {
        return Ok(None);
    }
    let repeated_start = at;
    let repeated = xref_some!(take_owned_reference(body, &mut at));
    if path_target
        .as_ref()
        .is_some_and(|target| !same_external_target(target, &repeated))
    {
        return Ok(None);
    }
    if path_target.is_none() {
        if cross_document_target(&repeated, ROOT_COMPONENT_TYPE_GUID).is_none() {
            return Ok(None);
        }
        link_names.push(xref_some!(repeated.link_name()).to_owned());
    }
    if repeated.target() == Some(0) {
        return Ok(None);
    }
    let role = xref_some!(repeated.link_name()).to_owned();
    let role_offset = xref_some!(reference_role_offset(body, repeated_start));
    let transform = match *xref_some!(body.get(at)) {
        1 => {
            at += 1;
            None
        }
        0 => {
            at += 1;
            let offset = at;
            let matrix = xref_some!(decode_rigid_matrix(body, at));
            at = xref_some!(at.checked_add(128));
            Some((offset, matrix))
        }
        _ => return Ok(None),
    };
    if xref_some!(View::u32_le_at(body, at)) != 0 {
        return Ok(None);
    }
    at += 4;
    let (final_role, next) = xref_some!(lp_utf16_bounded(body, at, 0..=256));
    if final_role != role {
        return Ok(None);
    }
    at = next;
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    let (target, type_guid) =
        xref_some!(xref_some!(take_owned_reference(body, &mut at)).into_local());
    Ok(
        (target != 0 && at == body.len()).then_some(RepeatedTargetPlacementDetails {
            occurrence_guid,
            carrier_record_index: xref_some!(View::u64_le_at(body, 7)),
            owner: XrefLocalOwner { target, type_guid },
            link_names,
            transform,
            role,
            role_offset,
        }),
    )
}

fn same_external_target(
    left: &Reference<String, String>,
    right: &Reference<String, String>,
) -> bool {
    match (left, right) {
        (
            Reference::CrossDocument {
                target: a,
                segment: sa,
                asset_guid: aa,
                target_type_guid: ta,
                link_name: ra,
                version: va,
                ..
            },
            Reference::CrossDocument {
                target: b,
                segment: sb,
                asset_guid: ab,
                target_type_guid: tb,
                link_name: rb,
                version: vb,
                ..
            },
        ) => {
            a == b
                && sa == sb
                && aa.eq_ignore_ascii_case(ab)
                && ta.eq_ignore_ascii_case(tb)
                && ra == rb
                && match (va, vb) {
                    (None, None) => true,
                    (Some((ka, ua)), Some((kb, ub))) => ka.eq_ignore_ascii_case(kb) && ua == ub,
                    _ => false,
                }
        }
        _ => false,
    }
}

fn reference_role_offset(body: &[u8], start: usize) -> Option<usize> {
    let mut at = start.checked_add(9)?;
    if View::u32_le_at(body, at) == Some(36) {
        at = lp_ascii_strict(body, at, 36..=36)?.1;
    }
    at = at.checked_add(5)?;
    at = lp_utf16_bounded_view(body, at, 0..=64)?.1.checked_add(1)?;
    if let Some((guid, end)) = lp_ascii_strict(body, at, 36..=36) {
        if crate::bytes::is_guid_hyphenated(guid) {
            at = end;
        }
    }
    at.checked_add(4)
}

/// Bind a repeated-target occurrence carrier to a Component Insert scope
/// through its relation record.
pub(crate) fn repeated_target_component_insert(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_transform: [[f64; 4]; 4],
) -> Result<Option<(String, usize, Option<usize>)>, CodecError> {
    let body = xref_some!(bytes.get(carrier_at..relation_at));
    if xref_some!(View::u64_le_at(body, 7)) != u64::from(carrier_record_index) {
        return Ok(None);
    }
    let details = xref_some!(repeated_target_occurrence_placement_details(body)?);
    let transform = details.transform.map_or(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        |(_, matrix)| matrix,
    );
    if transform != expected_transform {
        return Ok(None);
    }
    Ok(Some((
        details.role,
        carrier_at + details.role_offset,
        details.transform.map(|(offset, _)| carrier_at + offset),
    )))
}

/// Parse the grouped identity carrier used by the compact `Component Insert`
/// generation. The carrier has no matrix; its placement is the stored
/// identity transform. The repeated GUID and role fields are part of the
/// carrier grammar, not an occurrence-count signal.
pub(crate) fn grouped_component_insert_identity(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "382",
    )
}

/// Parse the class-380 grouped identity carrier used by the class-410/class-261
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class380(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "380",
    )
}

/// Parse the class-369 grouped identity carrier used by the class-426/class-258
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class369(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "369",
    )
}

/// Parse the variable-role grouped identity carrier used by the class-434/
/// class-266 `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class341(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "341",
    )
}

fn grouped_component_insert_identity_with_layout<'a>(
    bytes: &'a [u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_class_tag: &str,
) -> Option<(Utf16View<'a>, usize)> {
    const MARKER_AFTER_ROLE: &[u8] = &[0, 1, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_GUID_ROLE_MARKER: &[u8] = &[0, 3, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_EXTERNAL_ROLE_MARKER: &[u8] = &[0, 4, 0, 0, 0, 0, 1, 0, 0, 0];
    const MARKER_AFTER_METADATA: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    const MARKER_AFTER_PLACEMENT: &[u8] = &[0, 1, 0, 0, 0, 0];
    const CLASS_341_REPEAT_MARKER: &[u8] = &[1, 0, 0, 0, 0];
    const CLOSURE: &[u8] = &[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_span = relation_at.checked_sub(carrier_at)?;
    if class_tag != expected_class_tag
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
        || (expected_class_tag != "369"
            && expected_class_tag != "341"
            && carrier_span != grouped_identity_layout::LEN)
        || ((expected_class_tag == "369" || expected_class_tag == "341")
            && carrier_span < grouped_identity_layout::LEN)
        || bytes.get(carrier_at + 11..carrier_at + 19)? != [0; 8]
        || bytes.get(carrier_at + 19) != Some(&1)
        || bytes.get(carrier_at + 20..carrier_at + 24)? != [1, 0, 0, 0]
        || bytes.get(carrier_at + 24) != Some(&1)
        || bytes.get(carrier_at + 33) != Some(&1)
        || bytes.get(carrier_at + 34..carrier_at + 38)? != [0; 4]
    {
        return None;
    }
    let occurrence_identity = View::u64_le_at(
        bytes,
        carrier_at + grouped_identity_layout::OCCURRENCE_IDENTITY,
    )?;
    let (component_guid, mut at) = lp_utf16_bounded_view(
        bytes,
        carrier_at + grouped_identity_layout::FIRST_COMPONENT_GUID,
        36..=36,
    )?;
    if !component_guid.is_guid_relaxed() {
        return None;
    }
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    at = next;
    let first_role_at = at;
    let variable_role = matches!(expected_class_tag, "341" | "369");
    let role_bounds = if variable_role { 36..=256 } else { 36..=36 };
    let (role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    let valid_role = if variable_role {
        role.is_guid_relaxed() || role.is_guid_urn_role()
    } else {
        role.is_guid_relaxed()
    };
    if !valid_role {
        return None;
    }
    at = next;
    let marker_after_role = if expected_class_tag == "369" {
        if role.len() == 36 {
            CLASS_369_GUID_ROLE_MARKER
        } else {
            CLASS_369_EXTERNAL_ROLE_MARKER
        }
    } else {
        MARKER_AFTER_ROLE
    };
    if bytes.get(at..at + marker_after_role.len())? != marker_after_role {
        return None;
    }
    at += marker_after_role.len();

    let (metadata_guid_a, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_a.is_guid_relaxed() {
        return None;
    }
    at = next;
    let (metadata_guid_b, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_b.is_guid_relaxed() {
        return None;
    }
    at = next;
    if expected_class_tag == "341" {
        if bytes.get(at..at + 2)? != [0, 1]
            || View::u64_le_at(bytes, at + 2)? != occurrence_identity
        {
            return None;
        }
        at += 10;
        if bytes.get(at..at + CLASS_341_REPEAT_MARKER.len())? != CLASS_341_REPEAT_MARKER {
            return None;
        }
        at += CLASS_341_REPEAT_MARKER.len();
    } else {
        if bytes.get(at..at + MARKER_AFTER_METADATA.len())? != MARKER_AFTER_METADATA {
            return None;
        }
        at += MARKER_AFTER_METADATA.len();
    }

    let (repeated_component_guid, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !repeated_component_guid.is_guid_relaxed()
        || !repeated_component_guid.eq_ignore_ascii_case(component_guid)
    {
        return None;
    }
    at = next;
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (repeated_type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(repeated_type_guid) || !repeated_type_guid.eq_ignore_ascii_case(type_guid) {
        return None;
    }
    at = next;
    let (repeated_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    if !repeated_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + MARKER_AFTER_PLACEMENT.len())? != MARKER_AFTER_PLACEMENT {
        return None;
    }
    at += MARKER_AFTER_PLACEMENT.len();

    let (final_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds)?;
    if !final_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + CLOSURE.len())? != CLOSURE || at + CLOSURE.len() != relation_at {
        return None;
    }
    Some((role, first_role_at + 4))
}

/// Parse the current placement envelope: a standard target path, an optional
/// rigid matrix, and the generation-selected reference runs.
fn modern_occurrence_placement(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<OccurrencePlacement> {
    let (link_names, at) = occurrence_path(body)?;
    let ClosedPlacement { transform, .. } = placement_matrix_and_tail(body, at, serializer_magic)?;
    Some(OccurrencePlacement {
        frame_bytes: Vec::new(),
        occurrence_guid: None,
        carrier_record_index: View::u64_le_at(body, 7)?,
        owner: None,
        link_names,
        transform,
    })
}

/// The transform of a placement body that closes at its record end.
struct ClosedPlacement {
    /// `None` is the stored identity form.
    transform: Option<[[f64; 4]; 4]>,
    tail_offset: usize,
}

/// Read the optional identity marker, the matrix it selects, and the closing
/// reference runs, which must end at the record end.
fn placement_matrix_and_tail(
    body: &[u8],
    at: usize,
    serializer_magic: Option<u32>,
) -> Option<ClosedPlacement> {
    placement_matrix_and_tail_with(body, at, serializer_magic, decode_rigid_matrix)
}

/// Reads a 4x4 matrix at an offset and admits or refuses its linear block.
type MatrixDecoder = fn(&[u8], usize) -> Option<[[f64; 4]; 4]>;

/// [`placement_matrix_and_tail`] with the matrix admission chosen by the caller.
fn placement_matrix_and_tail_with(
    body: &[u8],
    at: usize,
    serializer_magic: Option<u32>,
    decode_matrix: MatrixDecoder,
) -> Option<ClosedPlacement> {
    // The identity marker is absent in the oldest container generation, which
    // always stores the matrix. Both readings start with a zero byte when the
    // marker is present and the matrix follows, so the record end decides.
    for identity_marker in [true, false] {
        let mut cursor = at;
        let mut transform = None;
        if identity_marker {
            match body.get(cursor) {
                Some(1) => cursor += 1,
                Some(0) => {
                    let Some(matrix) = decode_matrix(body, cursor + 1) else {
                        continue;
                    };
                    transform = Some(matrix);
                    cursor += 129;
                }
                _ => continue,
            }
        } else {
            let Some(matrix) = decode_matrix(body, cursor) else {
                continue;
            };
            transform = Some(matrix);
            cursor += 128;
        }
        if placement_tail(body, cursor, serializer_magic).is_some() {
            return Some(ClosedPlacement {
                transform,
                tail_offset: cursor,
            });
        }
    }
    None
}

struct ClosingOccurrenceChain<'a> {
    references: View<'a>,
    count: usize,
}

fn component_path_closing_chain(body: &[u8]) -> Option<ClosingOccurrenceChain<'_>> {
    let (_, at) = occurrence_path_members(body)?;
    if body.get(at) != Some(&0) {
        return None;
    }
    let placement = [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
        .into_iter()
        .find_map(|magic| {
            placement_matrix_and_tail_with(body, at + 1, magic, decode_orthonormal_matrix)
        })?;
    let mut view = View::over_retained(body);
    view.seek(placement.tail_offset)?;
    let count = view.u32_le()?;
    let count = view.counted(u64::from(count), 1)?.get();
    let start = view.position();
    let mut end = start;
    for _ in 0..count {
        take_reference(body, &mut end)?;
    }
    Some(ClosingOccurrenceChain {
        references: view.child(start, end)?,
        count,
    })
}

pub(crate) fn component_path_names_contained(body: &[u8], placement: u64, target: u64) -> bool {
    let Some((members, _)) = occurrence_path_members(body) else {
        return false;
    };
    let Some(elements) =
        ordered_contained_elements(&members).or_else(|| nested_contained_elements(&members))
    else {
        return false;
    };
    elements.placement_record_index == placement
        && elements.contained_record_index == target
        && component_path_closing_chain(body)
            .is_some_and(|chain| chain.names_contained(placement, target, elements.role))
}

impl ClosingOccurrenceChain<'_> {
    fn names_contained(&self, placement: u64, target: u64, role: &str) -> bool {
        if self.count != 2 {
            return false;
        }
        let mut at = 0;
        matches!(take_reference(self.references.window(), &mut at), Some(Reference::Local { target, inline_type_guid }) if target == placement && inline_type_guid.is_none_or(|guid| guid.eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)))
            && matches!(take_reference(self.references.window(), &mut at), Some(Reference::CrossDocument { target: record, target_type_guid, link_name, .. }) if record == target && target_type_guid.eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID) && link_name.eq_str(role))
    }
}

/// The current pose that a `CommonData` component-path carrier stores for one
/// occurrence placed at the document root
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentPathPose {
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// The occurrence-placement record that the path's local element names.
    pub(crate) placement_record_index: u64,
    /// The occurrence role of the path's cross-document element.
    pub(crate) role: String,
    /// `None` is the stored identity form.
    pub(crate) transform: Option<[[f64; 4]; 4]>,
    /// The complete carrier record, header included.
    pub(crate) frame_bytes: Vec<u8>,
}

/// Parse a component-path carrier whose path is one cross-document element and
/// one local element that names an occurrence-placement record, in either order.
pub(crate) fn component_path_pose(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<ComponentPathPose> {
    let (members, at) = occurrence_path_members(body)?;
    let [(first, _), (second, _)] = members.as_slice() else {
        return None;
    };
    // Both element orders occur: the cross-document element can precede or
    // follow the local element.
    let (role, placement_record_index) =
        [(first, second), (second, first)]
            .into_iter()
            .find_map(|(document, placement)| {
                Some((document.link_name()?, typed_local_placement(placement)?))
            })?;
    if placement_record_index == 0 || body.get(at) != Some(&0) {
        return None;
    }
    let ClosedPlacement { transform, .. } =
        placement_matrix_and_tail(body, at + 1, serializer_magic)?;
    Some(ComponentPathPose {
        carrier_record_index: View::u64_le_at(body, 7)?,
        placement_record_index,
        role: role.to_owned(),
        transform,
        frame_bytes: body.to_vec(),
    })
}

/// Type-table identity of a Design root-component aggregate, which a
/// component-path element names in a referenced design.
const ROOT_COMPONENT_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

/// The world pose that a `CommonData` component-path carrier stores for an
/// occurrence inside an external occurrence
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainedPathPose {
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// The occurrence role of the contained occurrence's placement.
    pub(crate) role: String,
    /// The root component record of the design that the contained occurrence places.
    pub(crate) component_record_index: u64,
    /// The contained occurrence's placement record in the referenced design.
    pub(crate) contained_record_index: u64,
    /// This document's occurrence-placement record of the external occurrence.
    pub(crate) placement_record_index: u64,
    /// The local occurrence carriers that hold the external occurrence, innermost first.
    pub(crate) owner_record_indices: Vec<u64>,
    /// `None` is the stored identity form.
    pub(crate) transform: Option<[[f64; 4]; 4]>,
    /// The complete carrier record, header included.
    pub(crate) frame_bytes: Vec<u8>,
}

/// Parse a component-path carrier of an occurrence inside an external occurrence.
/// The path is either two cross-document elements of one role (a root
/// component, then an occurrence placement in the referenced design), this
/// document's occurrence placement, and its local owners; or the root
/// component of a design that the referenced design places, this document's
/// occurrence placement, and that nested occurrence's placement in the
/// referenced design, with the document placement before or after the root
/// component.
pub(crate) fn contained_component_path_pose(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<ContainedPathPose> {
    let (members, at) = occurrence_path_members(body)?;
    let elements =
        ordered_contained_elements(&members).or_else(|| nested_contained_elements(&members))?;
    if body.get(at) != Some(&0) {
        return None;
    }
    let ClosedPlacement { transform, .. } =
        placement_matrix_and_tail(body, at + 1, serializer_magic)?;
    Some(ContainedPathPose {
        carrier_record_index: View::u64_le_at(body, 7)?,
        role: elements.role.to_owned(),
        component_record_index: elements.component_record_index,
        contained_record_index: elements.contained_record_index,
        placement_record_index: elements.placement_record_index,
        owner_record_indices: elements.owner_record_indices,
        transform,
        frame_bytes: body.to_vec(),
    })
}

/// Whether a retained contained pose is a carrier of a nested design.
pub(crate) fn contained_pose_is_nested(pose: &crate::records::xref::XrefContainedPose) -> bool {
    occurrence_path_members(&pose.frame_bytes)
        .is_some_and(|(members, _)| nested_contained_elements(&members).is_some())
}

pub(crate) fn component_path_has_inline_local_type(body: &[u8]) -> bool {
    occurrence_path_members(body).is_some_and(|(members, _)| {
        members
            .iter()
            .any(|(element, _)| element.local().is_some_and(|(_, guid)| guid.is_some()))
    })
}

/// The elements of a contained-occurrence component path.
struct ContainedElements<'a> {
    role: &'a str,
    component_record_index: u64,
    contained_record_index: u64,
    placement_record_index: u64,
    owner_record_indices: Vec<u64>,
}

/// The path of one role: root component, contained placement, this document's
/// placement, then its local owners, innermost first.
fn ordered_contained_elements(members: &PathMembers) -> Option<ContainedElements<'_>> {
    contained_elements(members, false)
}

fn nested_contained_elements(members: &PathMembers) -> Option<ContainedElements<'_>> {
    contained_elements(members, true)
}

fn contained_elements(members: &PathMembers, nested: bool) -> Option<ContainedElements<'_>> {
    let [(first, _), (second, _), (third, _), rest @ ..] = members.as_slice() else {
        return None;
    };
    if nested && first.local().is_none() && second.local().is_none() {
        return None;
    }
    let (component, contained, placement, owners) = if first.local().is_some() {
        if second.local().is_some() {
            let [(component, _), (contained, _)] = members.get(members.len().checked_sub(2)?..)?
            else {
                return None;
            };
            (
                component,
                contained,
                first,
                members.get(1..members.len() - 2)?,
            )
        } else {
            (second, third, first, rest)
        }
    } else if second.local().is_some() {
        (first, third, second, rest)
    } else {
        (first, second, third, rest)
    };
    let (component_record_index, component_role) =
        cross_document_target(component, ROOT_COMPONENT_TYPE_GUID)?;
    let (contained_record_index, role) =
        cross_document_target(contained, OCCURRENCE_PLACEMENT_TYPE_GUID)?;
    if (component_role != role) != nested {
        return None;
    }
    Some(ContainedElements {
        role,
        component_record_index,
        contained_record_index,
        placement_record_index: typed_local_placement(placement)?,
        owner_record_indices: owners
            .iter()
            .map(|(owner, _)| typed_local_placement(owner))
            .collect::<Option<Vec<_>>>()?,
    })
}

/// The target of a same-document occurrence-placement element, with or
/// without its inline type.
fn typed_local_placement(reference: &Reference<String, String>) -> Option<u64> {
    let (target, guid) = reference.local()?;
    (target != 0
        && crate::records::component_path::DesignComponentPathElementKind::Occurrence
            .admits_inline_type(guid))
    .then_some(target)
}

/// The target and link name of a cross-document element of one target type.
fn cross_document_target<'a>(
    reference: &'a Reference<String, String>,
    type_guid: &str,
) -> Option<(u64, &'a str)> {
    match reference {
        Reference::CrossDocument {
            target,
            inline_type_guid,
            target_type_guid,
            link_name,
            ..
        } if *target != 0
            && target_type_guid.eq_ignore_ascii_case(type_guid)
            && inline_type_guid
                .as_deref()
                .is_none_or(|inline| inline.eq_ignore_ascii_case(target_type_guid)) =>
        {
            Some((*target, link_name.as_str()))
        }
        _ => None,
    }
}

/// The world pose that a `CommonData` component-path carrier stores for an
/// external occurrence below local owners
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OwnedPathPose {
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// The occurrence role of the cross-document element.
    pub(crate) role: String,
    /// This document's occurrence-placement record of the external occurrence.
    pub(crate) placement_record_index: u64,
    /// The local occurrence carriers that hold it, innermost first.
    pub(crate) owner_record_indices: Vec<u64>,
    /// `None` is the stored identity form.
    pub(crate) transform: Option<[[f64; 4]; 4]>,
    /// The complete carrier record, header included.
    pub(crate) frame_bytes: Vec<u8>,
}

/// Parse a component-path carrier whose path is the referenced design's root
/// component (a cross-document element), this document's occurrence placement
/// of that role, and one or more of its local owners.
pub(crate) fn owned_component_path_pose(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<OwnedPathPose> {
    let (members, at) = occurrence_path_members(body)?;
    let [(component, _), (placement, _), owners @ ..] = members.as_slice() else {
        return None;
    };
    let (component, placement, owners) = if component.local().is_some() {
        if placement.local().is_some() {
            let (root, _) = members.last()?;
            (root, component, members.get(1..members.len() - 1)?)
        } else {
            (placement, component, owners)
        }
    } else {
        (component, placement, owners)
    };
    let (_, role) = cross_document_target(component, ROOT_COMPONENT_TYPE_GUID)?;
    let placement_record_index = typed_local_placement(placement)?;
    if owners.is_empty() || placement_record_index == 0 || body.get(at) != Some(&0) {
        return None;
    }
    let owner_record_indices = owners
        .iter()
        .map(|(owner, _)| typed_local_placement(owner))
        .collect::<Option<Vec<_>>>()?;
    let ClosedPlacement { transform, .. } =
        placement_matrix_and_tail(body, at + 1, serializer_magic)?;
    Some(OwnedPathPose {
        carrier_record_index: View::u64_le_at(body, 7)?,
        role: role.to_owned(),
        placement_record_index,
        owner_record_indices,
        transform,
        frame_bytes: body.to_vec(),
    })
}

/// Re-read a reference's retained owned poses through the carrier parser and
/// require each to name this reference's role and placement record.
pub(crate) fn owned_poses_match(reference: &XrefReference) -> bool {
    if reference.owned_poses.is_empty() {
        return true;
    }
    let Some(source) = &reference.placement_source else {
        return false;
    };
    reference.owned_poses.iter().all(|retained| {
        [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
            .into_iter()
            .filter_map(|magic| owned_component_path_pose(&retained.frame_bytes, magic))
            .any(|pose| {
                pose.carrier_record_index == retained.carrier_record_index
                    && pose.placement_record_index == source.carrier_record_index
                    && pose.role == reference.neutron_role.as_str()
                    && pose.owner_record_indices == retained.owner_record_indices
                    && pose.transform
                        == retained
                            .transform
                            .map(crate::records::xref::XrefPlacementTransform::rows)
            })
    })
}

/// Re-read a reference's retained contained poses through the carrier parser
/// and require each to name this reference's role and placement record.
pub(crate) fn contained_poses_match(reference: &XrefReference) -> bool {
    if reference.contained_poses.is_empty() {
        return true;
    }
    let Some(source) = &reference.placement_source else {
        return false;
    };
    reference.contained_poses.iter().all(|retained| {
        [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
            .into_iter()
            .filter_map(|magic| contained_component_path_pose(&retained.frame_bytes, magic))
            .any(|pose| {
                pose.carrier_record_index == retained.carrier_record_index
                    && pose.placement_record_index == source.carrier_record_index
                    && pose.role == reference.neutron_role.as_str()
                    && pose.contained_record_index == retained.contained_record_index
                    && pose.owner_record_indices == retained.owner_record_indices
                    && pose.transform
                        == retained
                            .transform
                            .map(crate::records::xref::XrefPlacementTransform::rows)
            })
    })
}

/// The current pose that a `CommonData` component-path carrier stores for one
/// local occurrence
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalComponentPathPose {
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// The component record that the path's first element names.
    pub(crate) component_record_index: u64,
    /// The local occurrence carrier that the path's second element names.
    pub(crate) occurrence_record_index: u64,
    /// `None` is the stored identity form.
    pub(crate) transform: Option<[[f64; 4]; 4]>,
    /// The complete carrier record, header included.
    pub(crate) frame_bytes: Vec<u8>,
}

/// Parse a component-path carrier whose path is two local elements: a
/// component record, then a local occurrence carrier of that component.
pub(crate) fn local_component_path_pose(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<LocalComponentPathPose> {
    let (members, at) = occurrence_path_members(body)?;
    let [(component, _), (occurrence, _)] = members.as_slice() else {
        return None;
    };
    let ((component_record_index, None), (occurrence_record_index, None)) =
        (component.local()?, occurrence.local()?)
    else {
        return None;
    };
    if component_record_index == 0 || occurrence_record_index == 0 || body.get(at) != Some(&0) {
        return None;
    }
    let ClosedPlacement { transform, .. } =
        placement_matrix_and_tail(body, at + 1, serializer_magic)?;
    Some(LocalComponentPathPose {
        carrier_record_index: View::u64_le_at(body, 7)?,
        component_record_index,
        occurrence_record_index,
        transform,
        frame_bytes: body.to_vec(),
    })
}

/// The local component-path poses of one Design stream, from the records its
/// type table registers as component paths.
pub(crate) fn local_component_path_poses(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    serializer_magic: Option<u32>,
) -> Result<Vec<LocalComponentPathPose>, CodecError> {
    Ok(
        typed_records(bytes, meta, "CommonData", COMPONENT_PATH_TYPE_GUID)?
            .iter()
            .filter_map(|record| bytes.get(record.offset..record.end))
            .filter_map(|body| local_component_path_pose(body, serializer_magic))
            .collect(),
    )
}

/// A component-path carrier whose path elements are all local records.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentPathCarrier {
    /// The carrier's own indexed record.
    pub(crate) carrier_record_index: u64,
    /// Record index and instance discriminator of each element, in path order.
    pub(crate) elements: Vec<(u64, u32)>,
    /// `None` is the stored identity form.
    pub(crate) transform: Option<[[f64; 4]; 4]>,
    /// The complete carrier record, header included.
    pub(crate) frame_bytes: Vec<u8>,
}

/// Parse a component-path carrier of any length whose elements are local
/// records without an inline type. The matrix is orthonormal; determinant
/// `-1` states a mirrored placement.
pub(crate) fn component_path_carrier(
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Option<ComponentPathCarrier> {
    let (members, at) = occurrence_path_members(body)?;
    let mut elements = Vec::with_capacity(members.len());
    for (element, discriminator) in &members {
        let (record_index, None) = element.local()? else {
            return None;
        };
        if record_index == 0 {
            return None;
        }
        elements.push((record_index, *discriminator));
    }
    if body.get(at) != Some(&0) {
        return None;
    }
    let ClosedPlacement { transform, .. } =
        placement_matrix_and_tail_with(body, at + 1, serializer_magic, decode_orthonormal_matrix)?;
    Some(ComponentPathCarrier {
        carrier_record_index: View::u64_le_at(body, 7)?,
        elements,
        transform,
        frame_bytes: body.to_vec(),
    })
}

/// The component-path carriers of one Design stream, from the records its
/// type table registers as component paths.
pub(crate) fn component_path_carriers(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    serializer_magic: Option<u32>,
) -> Result<Vec<ComponentPathCarrier>, CodecError> {
    Ok(
        typed_records(bytes, meta, "CommonData", COMPONENT_PATH_TYPE_GUID)?
            .iter()
            .filter_map(|record| bytes.get(record.offset..record.end))
            .filter_map(|body| component_path_carrier(body, serializer_magic))
            .collect(),
    )
}

/// Re-read a retained component-path record through the carrier parser and
/// require it to state the retained path and matrix.
pub(crate) fn component_path_matches(
    path: &crate::records::component_path::DesignComponentPath,
) -> bool {
    [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
        .into_iter()
        .filter_map(|magic| component_path_carrier(&path.frame_bytes, magic))
        .any(|carrier| {
            carrier.carrier_record_index == path.carrier_record_index
                && carrier.elements.len() == path.elements.len()
                && carrier.elements.iter().zip(&path.elements).all(
                    |((record, discriminator), element)| {
                        *record == element.record_index && *discriminator == element.discriminator
                    },
                )
                && carrier.transform
                    == path
                        .transform
                        .map(crate::records::component_path::ComponentPathTransform::rows)
        })
}

/// Re-read a local occurrence's retained current pose through the carrier
/// parser and require it to name this occurrence's component and carrier.
pub(crate) fn local_current_pose_matches(
    occurrence: &crate::records::feature::assembly_features::DesignComponentOccurrence,
) -> bool {
    let Some(current) = &occurrence.current_pose else {
        return true;
    };
    [None, Some(crate::metastream::MODERN_SERIALIZER_MAGIC)]
        .into_iter()
        .filter_map(|magic| local_component_path_pose(&current.frame_bytes, magic))
        .any(|pose| {
            pose.carrier_record_index == current.carrier_record_index
                && pose.component_record_index == occurrence.component_record_index
                && pose.occurrence_record_index == u64::from(occurrence.record_index)
                && pose.transform
                    == current
                        .transform
                        .map(crate::records::xref::XrefPlacementTransform::rows)
        })
}

pub(crate) fn identity_component_insert_carrier(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(String, usize)> {
    let body = bytes.get(carrier_at..relation_at)?;
    if View::u64_le_at(body, 7)? != u64::from(carrier_record_index) {
        return None;
    }
    let (mut at, targets_match) = legacy_occurrence_prefix(body, true)?;
    if !targets_match || body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let role_at = at + 4;
    let (role, next) = lp_utf16_bounded_view(body, at, 36..=36)?;
    if !role.is_guid_relaxed() || body.get(next..)? != [0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    Some((role.to_string(), carrier_at + role_at))
}

/// Parse the legacy typed placement envelope.
///
/// This form keeps the same stable type-table identity as the current
/// placement, but its target-reference carrier is wider and its matrix is
/// after the repeated target envelope. The dynamic class tag is deliberately
/// not an admission key: the type-table identity and exact member framing are
/// the stable discriminators.
fn legacy_occurrence_placement(body: &[u8]) -> Option<(Utf16View<'_>, Option<[[f64; 4]; 4]>)> {
    let (mut at, targets_match) = legacy_occurrence_prefix(body, false)?;
    if !targets_match {
        return None;
    }
    let identity_marker = *body.get(at)?;
    at += 1;
    let transform = match identity_marker {
        1 => None,
        0 => {
            let matrix = decode_rigid_matrix(body, at)?;
            at = at.checked_add(128)?;
            Some(matrix)
        }
        _ => return None,
    };
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, after_role) = lp_utf16_bounded_view(body, at, 36..=36)?;
    if !link_name.is_guid_relaxed() {
        return None;
    }
    at = after_role;
    if body.get(at..at + 12)? != [0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    at += 12;
    (at == body.len()).then_some((link_name, transform))
}

/// Return the role from a structurally valid legacy placement prefix.
///
/// The role is recovered even when the transform or closing tail is damaged,
/// so the caller can report an undecoded typed placement against the correct
/// external reference instead of treating it as an unrelated record.
fn legacy_occurrence_role(body: &[u8]) -> Option<Utf16View<'_>> {
    let (mut at, _) = legacy_occurrence_prefix(body, false)?;
    match *body.get(at)? {
        1 => at += 1,
        0 => at = at.checked_add(129)?,
        _ => return None,
    }
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, _) = lp_utf16_bounded_view(body, at, 36..=36)?;
    link_name.is_guid_relaxed().then_some(link_name)
}

/// Parse the shared prefix of the legacy identity and matrix forms.
fn legacy_occurrence_prefix(body: &[u8], insert: bool) -> Option<(usize, bool)> {
    let (_class_tag, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    let (name, after_name) = lp_ascii_strict(body, at, 0..=256)?;
    if insert && !name.is_empty() {
        return None;
    }
    at = after_name;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    let first_target = take_legacy_occurrence_reference(body, &mut at)?;
    if insert
        && !first_target
            .type_guid
            .eq_ignore_ascii_case("E03784ED-5E19-4E14-B9F2-3B07017018CD")
    {
        return None;
    }
    View::u32_le_at(body, at)?;
    at += 4;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    if insert {
        if !matches!(body.get(at), Some(0 | 1)) || body.get(at + 1) != Some(&0) {
            return None;
        }
        at += 2;
    }
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = lp_utf16_bounded_view(body, at, 36..=36)?;
        if !guid.is_guid_relaxed() {
            return None;
        }
        at = next;
    }
    if insert {
        if body.get(at) != Some(&1)
            || View::u64_le_at(body, at + 1)? == 0
            || body.get(at + 9..at + 16)? != [0, 1, 1, 0, 0, 0, 0]
        {
            return None;
        }
        at += 16;
    } else {
        if body.get(at) != Some(&0) {
            return None;
        }
        at += 1;
    }
    let second_target = take_legacy_occurrence_reference(body, &mut at)?;
    let targets_match = first_target.entity == second_target.entity
        && first_target.source_occurrence_identity == second_target.source_occurrence_identity
        && first_target
            .type_guid
            .eq_ignore_ascii_case(second_target.type_guid);
    Some((at, targets_match))
}

struct LegacyOccurrenceTarget<'a> {
    entity: u64,
    source_occurrence_identity: u64,
    type_guid: &'a str,
}

/// Consume one legacy occurrence target reference.
fn take_legacy_occurrence_reference<'a>(
    body: &'a [u8],
    at: &mut usize,
) -> Option<LegacyOccurrenceTarget<'a>> {
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    let entity = View::u64_le_at(body, *at)?;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    let source_occurrence_identity = View::u64_le_at(body, *at)?;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    let (type_guid, next) = lp_ascii_strict(body, *at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    *at = next;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    Some(LegacyOccurrenceTarget {
        entity,
        source_occurrence_identity,
        type_guid,
    })
}

/// Parse the target-path prefix shared by every occurrence-placement form.
fn occurrence_path(body: &[u8]) -> Option<(Vec<String>, usize)> {
    occurrence_target_path(body).map(|(names, _, end)| (names, end))
}

type OccurrenceTargetPath = (Vec<String>, Reference<String, String>, usize);

fn occurrence_target_path(body: &[u8]) -> Option<OccurrenceTargetPath> {
    let (names, target, at) = occurrence_path_elements(body)?;
    (body.get(at) == Some(&0)).then_some((names, target, at + 1))
}

fn occurrence_path_elements(body: &[u8]) -> Option<OccurrenceTargetPath> {
    let (members, at) = occurrence_path_members(body)?;
    let link_names = members
        .iter()
        .filter_map(|(element, _)| element.link_name().map(str::to_owned))
        .collect();
    let target = members.into_iter().last()?.0;
    Some((link_names, target, at))
}

/// Read the counted path elements with their instance discriminators, ending
/// before the path terminator.
/// Path elements with their instance discriminators, in path order.
type PathMembers = Vec<(Reference<String, String>, u32)>;

fn occurrence_path_members(body: &[u8]) -> Option<(PathMembers, usize)> {
    let (_, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    at = lp_ascii_strict(body, at, 0..=256)?.1;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    let count = View::u32_le_at(body, at).and_then(|count| usize::try_from(count).ok())?;
    if count == 0 || count > 4096 {
        return None;
    }
    at += 4;
    let mut members = Vec::new();
    for _ in 0..count {
        let element = take_owned_reference(body, &mut at)?;
        let discriminator = View::u32_le_at(body, at)?;
        at += 4;
        members.push((element, discriminator));
    }
    Some((members, at))
}

/// Consume the three reference runs that close a placement, returning `Some`
/// only when they end exactly at the record end.
fn placement_tail(body: &[u8], mut at: usize, serializer_magic: Option<u32>) -> Option<()> {
    let count = usize::try_from(View::u32_le_at(body, at)?).ok()?;
    if count > 256 {
        return None;
    }
    at += 4;
    for _ in 0..count {
        take_reference(body, &mut at)?;
    }
    // Only the modern MetaStream serializer magic admits the tagged run. Its
    // tag byte is neither reference presence value.
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        if matches!(body.get(at), Some(0 | 1)) {
            return None;
        }
        at += 1;
        let tagged = usize::try_from(View::u32_le_at(body, at)?).ok()?;
        if tagged > 256 {
            return None;
        }
        at = at.checked_add(4)?.checked_add(tagged.checked_mul(4)?)?;
        take_reference(body, &mut at)?;
    }
    take_reference(body, &mut at)?;
    take_reference(body, &mut at)?;
    (at == body.len()).then_some(())
}

/// A matrix whose linear block is orthonormal with determinant `1` or `-1`.
fn decode_orthonormal_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    crate::records::component_path::ComponentPathTransform::try_from(rows)
        .ok()
        .map(crate::records::component_path::ComponentPathTransform::rows)
}

fn decode_rigid_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    crate::records::xref::XrefPlacementTransform::try_from(rows)
        .ok()
        .map(crate::records::xref::XrefPlacementTransform::rows)
}

#[cfg(test)]
mod tests;
