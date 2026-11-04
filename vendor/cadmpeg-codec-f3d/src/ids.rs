// SPDX-License-Identifier: Apache-2.0
//! The `f3d:` URN identifier scheme.
//!
//! Segment vocabulary, separators, ordering, escaping, and `#{len}:{key}`
//! length-prefixes. Callers build IDs through the named functions below.

use crate::records::feature::combine::DesignCombineExternalBodyIdentity;

use crate::records::feature::{
    assembly::{DesignAssemblyAxialSelectorIdentity, DesignAssemblyLegacySelection},
    scope::DesignParameterScope,
};
use crate::records::{parameters::DesignParameter, sketch_placement::DesignSketchPlacement};

/// The scheme prefix shared by every `f3d:` URN. Used to strip or test the
/// scheme when parsing an identity key back into its stream and tail.
pub(crate) const SCHEME_PREFIX: &str = "f3d:";

/// Format component of every entity ID this codec emits.
pub(crate) const ID_FORMAT: cadmpeg_asm::ids::IdFormat = cadmpeg_asm::asm_format!("f3d");

/// The native stream used when an identity key carries no qualifying stream —
/// the fallback for `native_stream(id).unwrap_or(..)`.
pub(crate) const DEFAULT_STREAM: &str = "f3d:design";

/// Parse the native stream segment out of an identity key: the text before the
/// final `:` separator. Returns `None` when the key carries no separator.
pub(crate) fn native_stream(id: &str) -> Option<&str> {
    id.rsplit_once(':').map(|(stream, _)| stream)
}

/// Return whether two native IDs belong to the same root document or xref occurrence.
pub(crate) fn same_native_occurrence(left: &str, right: &str) -> bool {
    const OCCURRENCE_SEGMENT: &str = "/occurrence-";

    /// The segment holds no other `/`, so its matches are exactly the
    /// separators it starts at.
    fn segments(id: &str) -> impl Iterator<Item = usize> + '_ {
        id.match_indices('/')
            .map(|(at, _)| at)
            .filter(|&at| id[at..].starts_with(OCCURRENCE_SEGMENT))
    }

    fn occurrence(id: &str) -> Option<&str> {
        let mut occurrence_end = None;
        for at in segments(id) {
            let digits_at = at + OCCURRENCE_SEGMENT.len();
            let end = id[digits_at..]
                .find('/')
                .map_or(id.len(), |end| digits_at + end);
            if end > digits_at && id[digits_at..end].bytes().all(|byte| byte.is_ascii_digit()) {
                occurrence_end = Some(end);
            }
        }
        occurrence_end.map(|end| &id[..end])
    }

    match (occurrence(left), occurrence(right)) {
        (Some(left), Some(right)) => left == right,
        (None, None) => segments(left).next().is_none() && segments(right).next().is_none(),
        _ => false,
    }
}

/// Parse the Design segment shared by sibling `MetaStream.dat` and
/// `BulkStream.dat` entries from a native record identity.
pub(crate) fn design_segment(id: &str) -> Option<&str> {
    let stream = native_stream(id)?;
    let (segment, entry) = stream.rsplit_once('/')?;
    matches!(entry, "MetaStream.dat" | "BulkStream.dat").then_some(segment)
}

/// The fixed key of the single source-image record a design carries.
pub(crate) const FILE_SOURCE_IMAGE_ID: &str = "f3d:file:source-image#0";

/// The fixed key of the single design-intent failure record a contained
/// decode carries.
pub(crate) const DESIGN_INTENT_FAILURE_ID: &str = "f3d:design:intent-failure#0";

/// The fixed identity of the retained source image record.
pub(crate) fn file_source_image_id() -> cadmpeg_ir::ids::UnknownId {
    cadmpeg_ir::ids::UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "file", "source-image"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// The root product definition of a merged archive member.
pub(crate) fn archive_product_definition_id() -> cadmpeg_ir::ids::ProductDefinitionId {
    cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "archive", "product-definition"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// The neutral B-rep identity for one face slot.
pub(crate) fn brep_face_id(
    index: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::FaceId {
    cadmpeg_ir::ids::FaceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "brep", "entity"),
        index,
    )
}

/// Build an appearance identity from its source visual token.
pub(crate) fn appearance_id(key: cadmpeg_ir::ids::IdentityKey) -> cadmpeg_ir::ids::AppearanceId {
    cadmpeg_ir::ids::AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "design", "appearance"),
        key,
    )
}

/// Build a body appearance binding identity.
pub(crate) fn body_appearance_binding_id(
    entity_suffix: u64,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
) -> cadmpeg_ir::ids::AppearanceBindingId {
    let key = cadmpeg_ir::ids::IdentityKey::from(entity_suffix).colon(visual_guid);
    cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "body"),
        key,
    )
}

/// Build a body assignment appearance binding identity.
pub(crate) fn assignment_appearance_binding_id(
    entity_id: &str,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_ir::ids::IdentityError> {
    let entity = cadmpeg_ir::ids::IdentityKeyTail::try_new(entity_id)?;
    let key = cadmpeg_ir::identity_key!(":")
        .with_prefix(&entity)
        .then(visual_guid);
    Ok(cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "binding"),
        key,
    ))
}

/// Build a face appearance binding identity.
pub(crate) fn face_appearance_binding_id(
    face_guid: &str,
    visual_guid: cadmpeg_ir::ids::IdentityKey,
    face: &cadmpeg_ir::ids::FaceId,
) -> Result<cadmpeg_ir::ids::AppearanceBindingId, cadmpeg_ir::ids::IdentityError> {
    let face_guid = cadmpeg_ir::ids::IdentityKeyTail::try_new(face_guid)?;
    let face_key = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(face.as_str());
    let key = cadmpeg_ir::identity_key!(":")
        .with_prefix(&face_guid)
        .then(visual_guid)
        .colon(face_key.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&face_key);
    Ok(cadmpeg_ir::ids::AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "appearance", "face"),
        key,
    ))
}

/// Build a T-spline identity from its source entry key.
#[cfg(test)]
pub(crate) fn subd_id(
    source_key: &str,
) -> Result<cadmpeg_ir::ids::SubdId, cadmpeg_ir::ids::IdentityError> {
    let key = cadmpeg_ir::ids::IdentityKey::try_new(source_key)?;
    Ok(cadmpeg_ir::ids::SubdId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "tspline", "subd"),
        key,
    ))
}

/// Percent-encode identity separators, the escape byte, and whitespace.
pub(crate) fn identity_key_component(value: &str) -> String {
    cadmpeg_ir::ids::IdentityKeyTail::percent_encode(value)
        .as_str()
        .to_owned()
}

/// Reverse [`identity_key_component`] for a complete encoded component.
pub(crate) fn decode_identity_key_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    (|| {
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] != b'%' {
                decoded.push(bytes[at]);
                at += 1;
                continue;
            }
            let pair = bytes.get(at + 1..at + 3)?;
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            decoded.push(digit(pair[0])? * 16 + digit(pair[1])?);
            at += 3;
        }
        String::from_utf8(decoded).ok()
    })()
}

/// The neutral B-rep topology entity key for entity `index`.
pub(crate) fn brep_entity_id(index: impl std::fmt::Display) -> String {
    format!("f3d:brep:entity#{index}")
}

/// Neutral product occurrence projected from one external-reference placement.
pub(crate) fn neutral_xref_occurrence_id(
    reference_ordinal: u32,
    occurrence_ordinal: u32,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::identity_key!("xref")
            .dash(reference_ordinal)
            .dash(occurrence_ordinal),
    )
}

/// External occurrence identified by its complete containing instance path.
pub(crate) fn nested_xref_occurrence_id(
    parent: &cadmpeg_ir::ids::OccurrenceId,
    reference_ordinal: u32,
    occurrence_ordinal: u32,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::ids::Identity::from(parent.clone()).key().colon(
            cadmpeg_ir::identity_key!("xref")
                .dash(reference_ordinal)
                .dash(occurrence_ordinal),
        ),
    )
}

/// Neutral local component definition projected from its stable Design GUID.
pub(crate) fn neutral_component_id(
    guid: &crate::records::mesh::DesignRelaxedGuidText,
) -> cadmpeg_ir::ids::ProductDefinitionId {
    cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "component"),
        guid.identity_key(),
    )
}

/// Neutral local occurrence projected from its stable Design occurrence key.
pub(crate) fn neutral_component_occurrence_id(
    guid: &impl crate::records::mesh::OccurrenceIdentity,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        guid.occurrence_identity_key(),
    )
}

/// Neutral occurrence whose native key also occurs in another Design stream.
pub(crate) fn stream_component_occurrence_id(
    stream: &str,
    key: &impl crate::records::mesh::OccurrenceIdentity,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::identity_key!("stream")
            .with_hex_bytes(stream.as_bytes())
            .colon(key.occurrence_identity_key()),
    )
}

/// Neutral root occurrence for a context shared by several Design streams.
pub(crate) fn stream_root_occurrence_id(
    stream: &str,
    context: &crate::records::mesh::DesignRelaxedGuidText,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::identity_key!("stream")
            .with_hex_bytes(stream.as_bytes())
            .colon(cadmpeg_ir::identity_key!("root").dash(context.identity_key())),
    )
}

/// Neutral nested occurrence identified by its complete containing path.
pub(crate) fn nested_component_occurrence_id(
    parent: &cadmpeg_ir::ids::OccurrenceId,
    key: &impl crate::records::mesh::OccurrenceIdentity,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::ids::Identity::from(parent.clone())
            .key()
            .colon(key.occurrence_identity_key()),
    )
}

/// Neutral root occurrence of a Design stream, keyed by the root component's
/// naming-space UUID.
pub(crate) fn neutral_root_occurrence_id(
    context_uuid: &crate::records::mesh::DesignRelaxedGuidText,
) -> cadmpeg_ir::ids::OccurrenceId {
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        cadmpeg_ir::identity_key!("root").dash(context_uuid.identity_key()),
    )
}

/// Neutral occurrence identity for an external component-insert scope whose
/// target document is not present in the container.
pub(crate) fn neutral_component_insert_occurrence_id(
    scope: &DesignParameterScope,
) -> cadmpeg_ir::ids::OccurrenceId {
    let stream = identity_key_component(native_stream(&scope.id).unwrap_or(DEFAULT_STREAM));
    let stream_len = stream.len();
    let key = cadmpeg_ir::identity_key!("component-insert-")
        .then(stream_len)
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
            native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
        ))
        .then(scope.feature_ordinal.get())
        .colon(scope.record_index);
    cadmpeg_ir::ids::OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "occurrence"),
        key,
    )
}

/// Neutral assembly-joint key projected from one Design parameter scope.
pub(crate) fn neutral_assembly_joint_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> cadmpeg_ir::products::JointId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
        native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
    );
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(scope.record_index);
    cadmpeg_ir::products::JointId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "joint"),
        key,
    )
}

/// The joint that ties rigid-group member `member` (2 or more) to the group's
/// first member.
pub(crate) fn neutral_assembly_group_joint_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    member: usize,
) -> cadmpeg_ir::products::JointId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
        native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
    );
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(scope.record_index)
        .then(cadmpeg_ir::identity_key!("m"))
        .then(member);
    cadmpeg_ir::products::JointId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "joint"),
        key,
    )
}

pub(crate) fn feature_input_edge_slot(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    edge: &cadmpeg_ir::ids::HistoricalEdgeId,
) -> Option<i64> {
    let prefix = feature_input_prefix(feature, previous_state_id);
    let (_, key) = edge.as_str().split_once('#')?;
    let slot = key
        .strip_prefix(prefix.as_str())?
        .strip_prefix(':')?
        .parse::<i64>()
        .ok()
        .filter(|slot| *slot >= 0)?;
    history_input_edge_id(&prefix, slot)
        .eq(edge)
        .then_some(slot)
}

/// The history-input state of `feature_id` relative to `previous_state_id`.
pub(crate) fn feature_input_topology_id(
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    history_input_state_id(&feature_input_prefix(feature_id, previous_state_id))
}

/// The Design configuration record key for the archive entry `entry_name`.
pub(crate) fn configuration_entry_id(
    entry_name: &str,
    scope: &cadmpeg_ir::ids::IdentityComponent,
) -> String {
    format!(
        "f3d:{}:entry#{}",
        scope.as_str(),
        identity_key_component(entry_name)
    )
}

/// The neutral configuration key for table row `row_id` under `entry_name`,
/// with both length-prefixed into `#{len}:{key}{len}:{key}` segments.
pub(crate) fn neutral_configuration_id(
    entry_name: &str,
    row_id: &str,
) -> cadmpeg_ir::features::ConfigurationId {
    let entry_name = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(entry_name);
    let row_id = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(row_id);
    let key = cadmpeg_ir::ids::IdentityKey::from(entry_name.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&entry_name)
        .then(row_id.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&row_id);
    cadmpeg_ir::features::ConfigurationId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "configuration", "variant"),
        key,
    )
}

/// The equation container for document parameters in a merged member.
pub(crate) fn document_parameter_scope_id() -> cadmpeg_ir::features::FeatureId {
    cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "document-parameters"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// The neutral feature key for a parameter `scope`.
pub(crate) fn neutral_feature_id(scope: &DesignParameterScope) -> cadmpeg_ir::features::FeatureId {
    neutral_feature_id_parts(
        native_stream(&scope.id).unwrap_or(DEFAULT_STREAM),
        scope.source_kind_name(),
        scope.feature_ordinal.get(),
        scope.record_index,
    )
}

/// The neutral feature key from its `stream`, `kind`, ordinal, and scope record
/// index, with `stream` and `kind` length-prefixed into `#{len}:{key}` segments.
pub(crate) fn neutral_feature_id_parts(
    stream: &str,
    kind: &str,
    feature_ordinal: u32,
    scope_record_index: u32,
) -> cadmpeg_ir::features::FeatureId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream);
    let kind = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(kind);
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(kind.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&kind)
        .then(feature_ordinal)
        .colon(scope_record_index);
    cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "feature"),
        key,
    )
}

/// Feature-input-local body key for one complete external `Combine` selector path.
pub(crate) fn neutral_combine_external_body_id(
    identity: &DesignCombineExternalBodyIdentity,
) -> String {
    let selector_asset = identity_key_component(identity.selector_asset_id().as_str());
    let selector_context = identity_key_component(identity.selector_context_id().as_str());
    let external_asset = identity_key_component(identity.external_asset_id().as_str());
    let link_name = identity_key_component(identity.external_link_name());
    let property_key = identity
        .external_version()
        .map(|version| identity_key_component(version.property_key.value.as_str()))
        .unwrap_or_default();
    let version_urn = identity
        .external_version()
        .map(|version| identity_key_component(version.version_urn.value.as_str()))
        .unwrap_or_default();
    format!(
            "f3d:feature-input:body#combine-external:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            selector_asset.len(), selector_asset, selector_context.len(), selector_context,
            identity.occurrence_reference(), identity.external_body_reference(),
            identity.external_segment(), external_asset.len(), external_asset,
            link_name.len(), link_name, u8::from(identity.external_version().is_some()),
            property_key.len(), property_key,
            u8::from(identity.external_version().is_some()), version_urn.len(), version_urn,
        )
}

/// Feature-input-local connector key for one pathless axial assembly selector.
pub(crate) fn neutral_assembly_axial_object_id(
    identity: &DesignAssemblyAxialSelectorIdentity,
) -> String {
    let selector_asset =
        identity_key_component(&identity.selector_asset_id.as_str().to_ascii_lowercase());
    let selector_context =
        identity_key_component(&identity.selector_context_id.as_str().to_ascii_lowercase());
    let external_asset =
        identity_key_component(&identity.external_asset_id.as_str().to_ascii_lowercase());
    let link_name = identity_key_component(&identity.external_link_name);
    let property_key = identity
        .external_version
        .as_ref()
        .map(|version| version.property_key.value.as_str())
        .map(|value| identity_key_component(&value.to_ascii_lowercase()))
        .unwrap_or_default();
    let version_urn = identity
        .external_version
        .as_ref()
        .map(|version| version.version_urn.value.as_str())
        .map(identity_key_component)
        .unwrap_or_default();
    format!(
        "f3d:feature-input:connector#assembly-axial:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        selector_asset.len(),
        selector_asset,
        selector_context.len(),
        selector_context,
        identity.external_object_reference,
        identity.external_segment,
        external_asset.len(),
        external_asset,
        link_name.len(),
        link_name,
        u8::from(identity.external_version.is_some()),
        property_key.len(),
        property_key,
        u8::from(identity.external_version.is_some()),
        version_urn.len(),
        version_urn,
    )
}

/// Feature-input-local connector key for one direct legacy `As-built` face
/// selection.
pub(crate) fn neutral_assembly_legacy_object_id(
    selection: &DesignAssemblyLegacySelection,
) -> String {
    let asset = identity_key_component(&selection.asset_id.as_str().to_ascii_lowercase());
    let context = identity_key_component(&selection.context_id.as_str().to_ascii_lowercase());
    let recipe = identity_key_component(&selection.recipe_id.to_ascii_lowercase());
    format!(
        "f3d:feature-input:connector#assembly-legacy:{}:{}:{}:{}:{}:{}:{}:{}",
        asset.len(),
        asset,
        context.len(),
        context,
        recipe.len(),
        recipe,
        selection.record_index,
        selection.recipe_record_index,
    )
}

/// The neutral embedded-asset key for one exact archive entry: the encoded
/// entry name after its encoded byte length.
pub(crate) fn neutral_asset_id(entry_name: &str) -> cadmpeg_ir::assets::AssetId {
    let entry_name = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(entry_name);
    let key = cadmpeg_ir::ids::IdentityKey::from(entry_name.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&entry_name);
    cadmpeg_ir::assets::AssetId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "asset"),
        key,
    )
}

/// The neutral parameter key for a design `parameter`.
pub(crate) fn neutral_parameter_id(
    parameter: &DesignParameter,
) -> cadmpeg_ir::features::ParameterId {
    neutral_parameter_id_parts(
        native_stream(&parameter.id).unwrap_or(DEFAULT_STREAM),
        parameter.record_index,
    )
}

/// The neutral parameter key from its `stream` and indexed-record identity, with
/// `stream` length-prefixed into a `#{len}:{key}` segment.
pub(crate) fn neutral_parameter_id_parts(
    stream: &str,
    record_index: u32,
) -> cadmpeg_ir::features::ParameterId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream);
    let key = cadmpeg_ir::ids::IdentityKey::from(stream.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&stream)
        .then(record_index);
    cadmpeg_ir::features::ParameterId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "parameter"),
        key,
    )
}

/// The neutral planar-sketch key for a sketch `placement`.
pub(crate) fn neutral_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SketchId {
    cadmpeg_ir::sketches::SketchId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch"),
        sketch_placement_key(placement),
    )
}

/// The neutral spatial-sketch key for a sketch `placement`.
pub(crate) fn neutral_spatial_sketch_id(
    placement: &DesignSketchPlacement,
) -> cadmpeg_ir::sketches::SpatialSketchId {
    cadmpeg_ir::sketches::SpatialSketchId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch"),
        sketch_placement_key(placement),
    )
}

/// The shared body of a sketch or spatial-sketch placement key: the placement's
/// stream, escaped, joined to its entity suffix by `@`.
fn sketch_placement_key(placement: &DesignSketchPlacement) -> cadmpeg_ir::ids::IdentityKey {
    let stream = native_stream(&placement.id).unwrap_or(DEFAULT_STREAM);
    cadmpeg_ir::identity_key!("@")
        .then(placement.entity_id.suffix())
        .with_prefix(&cadmpeg_ir::ids::IdentityKeyTail::percent_encode(stream))
}

/// The neutral planar-sketch point-entity key under `sketch`.
pub(crate) fn neutral_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("p"),
            persistent_id,
        ),
    )
}

/// The neutral planar-sketch curve-entity key under `sketch`.
pub(crate) fn neutral_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_curve(sketch.as_str(), primary_id, secondary_id),
    )
}

/// The neutral planar-sketch text-entity key under `sketch`.
pub(crate) fn neutral_sketch_text_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("t"),
            persistent_id,
        ),
    )
}

/// The source-local neutral key for a planar sketch record that has no
/// persistent entity identity.
pub(crate) fn neutral_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchEntityId {
    cadmpeg_ir::sketches::SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("x"),
            u64::from(record_index),
        ),
    )
}

/// The neutral spatial-sketch curve-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_curve_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_curve(sketch.as_str(), primary_id, secondary_id),
    )
}

/// The neutral spatial-sketch point-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_point_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("p"),
            persistent_id,
        ),
    )
}

/// The source-local neutral key for a spatial-sketch record that has no
/// persistent entity identity.
pub(crate) fn neutral_spatial_sketch_record_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    record_index: u32,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("x"),
            u64::from(record_index),
        ),
    )
}

/// The neutral spatial-sketch surface-entity key under `sketch`.
pub(crate) fn neutral_spatial_sketch_surface_id(
    sketch: &cadmpeg_ir::sketches::SpatialSketchId,
    persistent_id: u64,
) -> cadmpeg_ir::sketches::SpatialSketchEntityId {
    cadmpeg_ir::sketches::SpatialSketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "spatial-sketch-entity"),
        sketch_entity_tagged(
            sketch.as_str(),
            cadmpeg_ir::identity_key!("s"),
            persistent_id,
        ),
    )
}

/// A single-tag sketch-entity key: the escaped owning-sketch key, length-
/// prefixed, followed by a one-character tag (`p`/`t`/`s`/`x`) and one id.
fn sketch_entity_tagged(
    sketch_key: &str,
    tag: cadmpeg_ir::ids::IdentityKey,
    id: u64,
) -> cadmpeg_ir::ids::IdentityKey {
    let sketch = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(sketch_key);
    cadmpeg_ir::ids::IdentityKey::from(sketch.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&sketch)
        .then(tag)
        .then(id)
}

/// A curve sketch-entity key: the escaped owning-sketch key, length-prefixed,
/// followed by `c`, the primary id, and the colon-joined secondary id.
fn sketch_entity_curve(
    sketch_key: &str,
    primary_id: u64,
    secondary_id: u64,
) -> cadmpeg_ir::ids::IdentityKey {
    let sketch = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(sketch_key);
    cadmpeg_ir::ids::IdentityKey::from(sketch.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .with_tail(&sketch)
        .then(cadmpeg_ir::identity_key!("c"))
        .then(primary_id)
        .colon(secondary_id)
}

/// The neutral sketch-constraint key for `native_ref` at `record_index`.
pub(crate) fn neutral_sketch_constraint_id(
    native_ref: &str,
    record_index: u32,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let stream = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(
        native_stream(native_ref).unwrap_or(DEFAULT_STREAM),
    );
    cadmpeg_ir::sketches::SketchConstraintId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-constraint"),
        cadmpeg_ir::identity_key!("@")
            .then(record_index)
            .with_prefix(&stream),
    )
}

/// The neutral dimension-constraint key derived from a `parameter` key and a
/// dimension `form`, with the parameter key tail and form length-prefixed.
pub(crate) fn neutral_dimension_constraint_id(
    parameter: &cadmpeg_ir::features::ParameterId,
    form: &str,
) -> cadmpeg_ir::sketches::SketchConstraintId {
    let parameter_key = parameter.key();
    let form = cadmpeg_ir::ids::IdentityKeyTail::percent_encode(form);
    cadmpeg_ir::sketches::SketchConstraintId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "model", "sketch-constraint"),
        cadmpeg_ir::identity_key!("dimension")
            .colon(parameter_key.as_str().len())
            .colon(parameter_key)
            .then(form.as_str().len())
            .then(cadmpeg_ir::identity_key!(":"))
            .with_tail(&form),
    )
}

// --- history-input topology keys -------------------------------------------
//
// A history-input key names a feature's boundary topology relative to a prior
// history state. The shared body is `{len}:{feature_key}:{previous_state_id}`;
// the entity kinds (edge/face/body) append `:{slot}`, and the state key stops
// at the body.

/// The shared body of a history-input key: the feature key length-prefixed and
/// joined to `previous_state_id` by colons.
pub(crate) fn history_input_prefix(
    feature_key: &cadmpeg_ir::ids::IdentityKey,
    previous_state_id: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::ids::IdentityKey::from(feature_key.as_str().len())
        .then(cadmpeg_ir::identity_key!(":"))
        .then(feature_key)
        .colon(previous_state_id)
}

/// The [`history_input_prefix`] of `feature`'s key relative to
/// `previous_state_id`.
pub(crate) fn feature_input_prefix(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::IdentityKey {
    history_input_prefix(&feature.key(), previous_state_id)
}

/// The history-input state key for a `prefix` from [`history_input_prefix`].
pub(crate) fn history_input_state_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    cadmpeg_ir::ids::FeatureInputTopologyId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "state"),
        prefix.clone(),
    )
}

/// The history-input edge key for `slot` under a `prefix`.
pub(crate) fn history_input_edge_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalEdgeId {
    cadmpeg_ir::ids::HistoricalEdgeId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "edge"),
        prefix.clone().colon(slot),
    )
}

/// The history-input vertex key for `slot` under a `prefix`.
pub(crate) fn history_input_vertex_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalVertexId {
    cadmpeg_ir::ids::HistoricalVertexId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "vertex"),
        prefix.clone().colon(slot),
    )
}

/// The history-input face key for `slot` under a `prefix`.
pub(crate) fn history_input_face_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalFaceId {
    cadmpeg_ir::ids::HistoricalFaceId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "face"),
        prefix.clone().colon(slot),
    )
}

/// The history-input body key for `slot` under a `prefix`.
pub(crate) fn history_input_body_id(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slot: impl Into<cadmpeg_ir::ids::IdentityKey>,
) -> cadmpeg_ir::ids::HistoricalBodyId {
    cadmpeg_ir::ids::HistoricalBodyId::compose(
        &cadmpeg_ir::identity_namespace!("f3d", "history-input", "body"),
        prefix.clone().colon(slot),
    )
}

// --- native design-record keys ---------------------------------------------
//
// Native design records are keyed `f3d:{scope}:{kind}#{offset}`, where `scope`
// is the escaped archive stream name and `offset` is the record's byte offset
// or index within that stream.

/// The native scope key for an archive entry or stream `name`.
pub(crate) fn native_scope(name: &str) -> String {
    format!("f3d:{}", identity_key_component(name))
}

/// Compare an encoded identity component with source text without allocating.
pub(crate) fn encoded_identity_key_component_matches(encoded: &str, source: &str) -> bool {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut actual = encoded.as_bytes().iter();
    for character in source.chars() {
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            for &byte in bytes {
                for expected in [
                    b'%',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 0x0f)],
                ] {
                    if actual.next() != Some(&expected) {
                        return false;
                    }
                }
            }
        } else {
            for byte in bytes {
                if actual.next() != Some(byte) {
                    return false;
                }
            }
        }
    }
    actual.next().is_none()
}

/// Compare an escaped native scope without materializing the encoded entry name.
pub(crate) fn native_scope_matches(stream: &str, entry: &str) -> bool {
    fn encoded_len(source: &str) -> Option<usize> {
        source.chars().try_fold(0usize, |length, character| {
            let width = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                character.len_utf8().checked_mul(3)?
            } else {
                character.len_utf8()
            };
            length.checked_add(width)
        })
    }

    let Some(encoded_length) = encoded_len(entry) else {
        return false;
    };
    if let Some(direct) = stream.strip_prefix("f3d:") {
        if direct.len() == encoded_length && encoded_identity_key_component_matches(direct, entry) {
            return true;
        }
    }
    stream
        .strip_prefix("f3d:xref/")
        .and_then(|qualified| qualified.strip_suffix(entry))
        .is_some_and(|prefix| prefix.ends_with('/'))
}

/// Build one record ID in an archive-entry-qualified native scope.
pub(crate) fn native_scoped_id(scope: &str, kind: &str, key: impl std::fmt::Display) -> String {
    format!("{}:{kind}#{key}", native_scope(scope))
}

/// Macro defining one `f3d:{scope}:{kind}#{offset}` native-record builder.
macro_rules! native_record_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        pub(crate) fn $name(scope: &str, offset: impl std::fmt::Display) -> String {
            native_scoped_id(scope, $kind, offset)
        }
    };
}

native_record_id!(
    /// The native design-parameter record key.
    native_design_parameter_id,
    "design-parameter"
);
native_record_id!(
    /// The native design-parameter-owner record key.
    native_design_parameter_owner_id,
    "design-parameter-owner"
);
native_record_id!(
    /// The native ordered Design feature-timeline record key.
    #[cfg(test)]
    native_design_feature_timeline_id,
    "design-feature-timeline"
);
native_record_id!(
    /// The native Design component naming-space binding key.
    #[cfg(test)]
    native_design_component_naming_space_id,
    "design-component-naming-space"
);

/// The native ordered Design feature-timeline key in an already encoded
/// `f3d:` stream scope.
#[cfg(test)]
pub(crate) fn native_design_feature_timeline_id_in_stream(
    stream: &str,
    offset: impl std::fmt::Display,
) -> String {
    format!("{stream}:design-feature-timeline#{offset}")
}
native_record_id!(
    /// The native design Canvas image-plane binding key.
    native_design_canvas_image_id,
    "design-canvas-image"
);
native_record_id!(
    /// The native design Decal image and target binding key.
    native_design_decal_image_id,
    "design-decal-image"
);
native_record_id!(
    /// The native persistent-reference record key.
    #[cfg(test)]
    native_persistent_reference_id,
    "persistent-reference"
);
native_record_id!(
    /// The native lost-edge-reference record key.
    #[cfg(test)]
    native_lost_edge_reference_id,
    "lost-edge-reference"
);
native_record_id!(
    /// The native design-type record key.
    #[cfg(test)]
    native_design_type_id,
    "design-type"
);
native_record_id!(
    /// The native design-record-header record key.
    #[cfg(test)]
    native_design_record_header_id,
    "design-record-header"
);
native_record_id!(
    /// The native sketch-surface record key.
    native_sketch_surface_id,
    "sketch-surface"
);
native_record_id!(
    /// The native mesh-body record key.
    #[cfg(test)]
    native_mesh_body_id,
    "mesh-body"
);
native_record_id!(
    /// The native Design mesh-feature graph key.
    #[cfg(test)]
    native_design_mesh_feature_id,
    "design-mesh-feature"
);
native_record_id!(
    /// The native design-body-member record key.
    #[cfg(test)]
    native_design_body_member_id,
    "design-body-member"
);
native_record_id!(
    /// The native design-body-binding record key.
    native_design_body_binding_id,
    "design-body-binding"
);

#[cfg(test)]
mod tests {
    use super::{
        decode_identity_key_component, design_segment, face_appearance_binding_id,
        native_design_feature_timeline_id_in_stream, native_design_type_id, native_scope,
        neutral_assembly_legacy_object_id, neutral_sketch_record_id, neutral_sketch_text_id,
        same_native_occurrence, SCHEME_PREFIX,
    };
    use crate::records::{
        feature::assembly::DesignAssemblyLegacySelection, recipes::ConstructionRecipeKind,
    };

    #[test]
    fn native_scoped_id_matches_escaped_identity_bytes() {
        let scope = "A B:#%\u{2003}é";
        let actual = super::native_scoped_id(scope, "act-guid", 42);
        assert_eq!(actual, super::native_scoped_id(scope, "act-guid", 42));
    }

    #[test]
    fn native_scope_match_preserves_direct_and_xref_comparisons() {
        for entry in ["Design/BulkStream.dat", "A B:#%\u{2003}é", ""] {
            let direct = super::native_scope(entry);
            assert!(super::native_scope_matches(&direct, entry));
            assert!(!super::native_scope_matches(&direct, "different"));
            let xref = format!("f3d:xref/part/{entry}");
            assert!(super::native_scope_matches(&xref, entry));
            assert!(!super::native_scope_matches(&format!("{xref}extra"), entry));
        }
    }

    #[test]
    fn history_keys_preserve_admitted_colons_percent_escapes_and_signed_states() {
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#a:b%20c").unwrap();
        let prefix = super::history_input_prefix(&feature.key(), -3);
        assert_eq!(prefix.as_str(), "7:a:b%20c:-3");
        assert_eq!(
            super::history_input_edge_id(&prefix, 9).as_str(),
            "f3d:history-input:edge#7:a:b%20c:-3:9"
        );
    }

    #[test]
    fn history_input_edge_slots_use_the_feature_and_state_key() {
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet").unwrap();
        let edge = super::history_input_edge_id(&super::feature_input_prefix(&feature, 7), 17);
        assert_eq!(super::feature_input_edge_slot(&feature, 7, &edge), Some(17));
        assert_eq!(super::feature_input_edge_slot(&feature, 8, &edge), None);
        let other = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#other").unwrap();
        assert_eq!(super::feature_input_edge_slot(&other, 7, &edge), None);
        let escaped = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#a:b%20c").unwrap();
        let edge = super::history_input_edge_id(&super::feature_input_prefix(&escaped, -3), 17);
        assert_eq!(
            super::feature_input_edge_slot(&escaped, -3, &edge),
            Some(17)
        );
    }

    #[test]
    fn history_input_edge_slots_refuse_truncated_or_incompatible_keys() {
        let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet").unwrap();
        for key in [
            "f3d:history-input:edge#6:fillet:7",
            "f3d:history-input:edge#6:fillet:7:",
            "f3d:history-input:edge#6:fillet:7:17:18",
            "f3d:history-input:edge#6:fillet:7:-1",
            "f3d:history-input:edge#6:fillet:7:017",
            "f3d:brep:entity#6:fillet:7:17",
            "f3d:history-input:edge#17",
        ] {
            let edge = cadmpeg_ir::ids::HistoricalEdgeId::mint(key).unwrap();
            assert_eq!(super::feature_input_edge_slot(&feature, 7, &edge), None);
        }
    }

    #[test]
    fn empty_stream_remains_empty_before_the_sketch_entity_suffix() {
        let placement = crate::records::sketch_placement::DesignSketchPlacement {
            id: ":record#4".into(),
            scope_record_index: None,
            entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 7),
            visibility: None,
            class_tag: "330".to_owned().try_into().unwrap(),
            record_index: 4,
            paired_class_tag: "330".to_owned().try_into().unwrap(),
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                0,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
            )
            .unwrap(),
        };
        assert_eq!(
            super::neutral_sketch_id(&placement).as_str(),
            "f3d:model:sketch#@7"
        );
        assert_eq!(
            super::neutral_sketch_constraint_id(&placement.id, 4).as_str(),
            "f3d:model:sketch-constraint#@4"
        );
    }

    #[test]
    fn admitted_guid_and_visual_tokens_retain_source_spelling() {
        let text = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
        let guid: crate::records::mesh::DesignRelaxedGuidText = text.to_owned().try_into().unwrap();
        assert_eq!(
            super::neutral_component_id(&guid).as_str(),
            "f3d:model:component#aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
        );
        assert_eq!(
            serde_json::to_string(&guid).unwrap(),
            serde_json::to_string(text).unwrap()
        );
        let token_text = format!("{text}_Post2015_Post2015");
        let token: crate::records::references::DesignVisualToken =
            token_text.clone().try_into().unwrap();
        assert_eq!(
            super::appearance_id(token.identity_key()).as_str(),
            format!("f3d:design:appearance#{token_text}")
        );
        assert_eq!(
            serde_json::to_string(&token).unwrap(),
            serde_json::to_string(&token_text).unwrap()
        );
        assert!(crate::records::mesh::DesignRelaxedGuidText::try_from(String::new()).is_err());
        assert!(crate::records::references::DesignVisualToken::try_from(String::new()).is_err());
    }

    #[test]
    fn raw_appearance_components_keep_legal_separators_and_reject_whitespace() {
        let id = super::assignment_appearance_binding_id(
            "part:one%20_7",
            cadmpeg_ir::identity_key!("visual"),
        )
        .unwrap();
        assert_eq!(id.as_str(), "f3d:appearance:binding#part:one%20_7:visual");
        assert!(super::assignment_appearance_binding_id(
            "part one_7",
            cadmpeg_ir::identity_key!("visual")
        )
        .is_err());
        assert_eq!(
            super::subd_id("cage:one%20").unwrap().as_str(),
            "f3d:tspline:subd#cage:one%20"
        );
    }

    #[test]
    fn design_segment_joins_sibling_meta_and_bulk_stream_ids() {
        let meta = "f3d:Asset/Design1/MetaStream.dat:design-type#10";
        let bulk = "f3d:Asset/Design1/BulkStream.dat:design-canvas-image#20";
        assert_eq!(design_segment(meta), Some("f3d:Asset/Design1"));
        assert_eq!(design_segment(meta), design_segment(bulk));
        assert_eq!(
            design_segment("f3d:Asset/Design1/Other.dat:record#20"),
            None
        );
    }

    #[test]
    fn native_ids_escape_archive_names_without_losing_the_raw_stream() {
        let entry = "Simulation Case/Design:1/MetaStream%20.dat";
        let id = native_design_type_id(entry, 10);
        assert_eq!(
            id,
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-type#10"
        );
        let encoded = native_scope(entry)
            .strip_prefix(SCHEME_PREFIX)
            .expect("native scheme")
            .to_owned();
        assert_eq!(
            decode_identity_key_component(&encoded).as_deref(),
            Some(entry)
        );
        assert_eq!(
            crate::writer::patch::records::native_stream(&id, ":design-type#")
                .expect("writer stream"),
            entry
        );
        assert_eq!(
            native_design_feature_timeline_id_in_stream(&native_scope(entry), 20),
            "f3d:Simulation%20Case/Design%3A1/MetaStream%2520.dat:design-feature-timeline#20"
        );
    }

    #[test]
    fn face_appearance_binding_escapes_the_nested_face_identity() {
        let id = face_appearance_binding_id(
            "face-guid",
            cadmpeg_ir::identity_key!("visual-guid"),
            &cadmpeg_ir::ids::FaceId::mint("f3d:brep/path:face#12").expect("identity grammar"),
        )
        .expect("valid test identity");
        assert_eq!(
            id.as_str(),
            "f3d:appearance:face#face-guid:visual-guid:27:f3d%3Abrep/path%3Aface%2312"
        );
        assert_eq!(id.as_str().matches('#').count(), 1);
    }

    #[test]
    fn native_occurrence_scope_isolates_xrefs_and_includes_root_streams() {
        assert!(same_native_occurrence(
            "f3d:Asset/Design1/BulkStream.dat:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
        assert!(same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/Asset/Design1/BulkStream.dat:record#1",
            "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-0/xref/child/occurrence-0/design:record#1",
            "f3d:xref/root/occurrence-0/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-invalid/design:record#1",
            "f3d:design:persistent-subentity-tag#1",
        ));
    }

    #[test]
    fn native_occurrence_scope_ends_at_the_last_numbered_segment() {
        assert!(same_native_occurrence(
            "f3d:xref/root/occurrence-1/xref/child/occurrence-/design:record#1",
            "f3d:xref/root/occurrence-1/design:persistent-subentity-tag#1",
        ));
        assert!(!same_native_occurrence(
            "f3d:xref/root/occurrence-1/xref/child/occurrence-2/design:record#1",
            "f3d:xref/root/occurrence-1/design:persistent-subentity-tag#1",
        ));
    }

    #[test]
    fn identityless_sketch_geometry_uses_a_disjoint_source_record_namespace() {
        let sketch = cadmpeg_ir::sketches::SketchId::mint("f3d:model:sketch#example").unwrap();
        let persistent = neutral_sketch_text_id(&sketch, 42);
        let source_record = neutral_sketch_record_id(&sketch, 42);
        assert_ne!(persistent, source_record);
        assert_eq!(source_record, neutral_sketch_record_id(&sketch, 42));
        assert_ne!(source_record, neutral_sketch_record_id(&sketch, 43));
    }

    #[test]
    fn legacy_assembly_connector_key_is_namespace_and_recipe_scoped() {
        let selection = DesignAssemblyLegacySelection {
            record_index: 7,
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 110,
            context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 120,
            recipe_record_index: 8,
            recipe_record_byte_offset: 130,
            recipe_id: "Recipe:1".into(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_references: Vec::new(),
            next_byte_offset: 140,
        };
        assert_eq!(
            neutral_assembly_legacy_object_id(&selection),
            "f3d:feature-input:connector#assembly-legacy:36:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:36:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:10:recipe%3A1:7:8"
        );
        let mut second = selection.clone();
        second.record_index += 1;
        assert_ne!(
            neutral_assembly_legacy_object_id(&selection),
            neutral_assembly_legacy_object_id(&second)
        );
    }
}
