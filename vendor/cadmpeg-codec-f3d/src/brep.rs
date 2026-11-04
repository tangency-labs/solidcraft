// SPDX-License-Identifier: Apache-2.0
//! Wrap the format-independent ASM B-rep graph with Fusion-derived links,
//! blob-scoped id qualification, and Design body-map selector resolution.

use crate::records::{
    recipes::CreationTimestamp,
    sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink},
};
use cadmpeg_asm::brep::attributes::attribute_key;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::{decode_with_header, decode_with_purpose, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::bounded_len;
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

mod graph_ops;

fn merge_brep_counts(target: &mut BTreeMap<String, usize>, source: BTreeMap<String, usize>) {
    for (kind, count) in source {
        *target.entry(kind).or_default() += count;
    }
}

/// The ASM B-rep graph plus links derived from Fusion attribute records.
///
/// Kernel arenas form the top-level structural projection.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Brep {
    /// The format-independent ASM graph.
    #[serde(flatten)]
    pub(crate) asm: AsmBrep,
    /// Typed sketch-curve provenance links.
    pub(crate) sketch_curve_links: Vec<SketchCurveLink>,
    /// Persistent design identifiers attached to solved entities.
    pub(crate) persistent_design_links: Vec<PersistentDesignLink>,
    /// Variable-width persistent tag groups attached to solved faces and edges.
    pub(crate) persistent_subentity_tags: Vec<PersistentSubentityTag>,
    /// Original authoring times attached to solved entities.
    pub(crate) creation_timestamps: Vec<CreationTimestamp>,
    /// Sketch-region bodies removed from keyless blobs.
    #[serde(skip)]
    pub(crate) sketch_region_bodies_omitted: usize,
}

impl Brep {
    /// Wrap a decoded ASM graph and derive the Fusion attribute links.
    fn from_asm(asm: AsmBrep) -> Result<Self, CodecError> {
        let mut sketch_curve_links = Vec::new();
        let mut persistent_design_links = Vec::new();
        let mut persistent_subentity_tags = Vec::new();
        let mut creation_timestamps = Vec::new();
        for attribute in &asm.attributes {
            if let Some(link) = sketch_curve_link(attribute) {
                sketch_curve_links.push(link);
            }
            for link in self::persistent_design_links(attribute)? {
                persistent_design_links.push(link);
            }
            for tag in self::persistent_subentity_tags(attribute)? {
                persistent_subentity_tags.push(tag);
            }
            if let Some(timestamp) = creation_timestamp(attribute) {
                creation_timestamps.push(timestamp);
            }
        }
        Ok(Self {
            asm,
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
            sketch_region_bodies_omitted: 0,
        })
    }

    /// Map solved bodies to the selector used by this blob's Design body map.
    pub(crate) fn body_selectors(&self) -> HashMap<BodyId, u64> {
        let ordinal_mode = self
            .asm
            .body_native_keys
            .iter()
            .all(|body| body.asm_body_key.is_none());
        let mut selectors = HashMap::new();
        for body in &self.asm.body_native_keys {
            let selector = if ordinal_mode {
                Some(u64::from(body.body_ordinal))
            } else {
                body.asm_body_key
            };
            let Some(selector) = selector else {
                continue;
            };
            selectors.insert(body.body.clone(), selector);
        }
        selectors
    }

    /// Resolve the Design selectors present for this blob. An exact native
    /// body key has precedence. A selector absent from the native-key domain
    /// selects the body with the same zero-based ordinal.
    pub(crate) fn body_selectors_for(
        &self,
        selectors: &HashSet<u64>,
    ) -> Result<HashMap<BodyId, u64>, cadmpeg_core::CodecError> {
        let mut resolved = HashMap::new();
        for selector in selectors {
            let Some(body) = resolve_body_selector(self.asm.body_native_keys.iter(), *selector)?
            else {
                continue;
            };
            if let Some(previous) = resolved.insert(body.clone(), *selector) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "F3D body {} is selected by both {previous} and {selector}",
                    body.as_str()
                )));
            }
        }
        Ok(resolved)
    }

    /// Append a disjoint, already-qualified BREP graph.
    pub(crate) fn append(&mut self, other: Self) {
        self.sketch_region_bodies_omitted += other.sketch_region_bodies_omitted;
        let mut asm = other.asm;
        macro_rules! append_vecs {
            ($($field:ident),+ $(,)?) => {
                $(self.asm.$field.append(&mut asm.$field);)+
            };
        }
        append_vecs!(
            bodies,
            regions,
            shells,
            faces,
            loops,
            coedges,
            edges,
            vertices,
            points,
            surfaces,
            curves,
            pcurves,
            procedural_surfaces,
            procedural_curves,
            edge_continuities,
            edge_ownerships,
            vertex_ownerships,
            face_sidedness,
            face_native_keys,
            tolerant_coedge_parameters,
            tolerant_edge_tails,
            tolerant_vertex_tails,
            mesh_surface_sentinels,
            transform_hints,
            body_native_keys,
            wire_topologies,
            attributes,
            unknowns,
            annotation_records,
        );
        self.asm.stats.mesh_surface_faces += asm.stats.mesh_surface_faces;
        self.asm.stats.nurbs_surfaces += asm.stats.nurbs_surfaces;
        self.asm.stats.nurbs_curves += asm.stats.nurbs_curves;
        self.asm.stats.partial_procedural_supports += asm.stats.partial_procedural_supports;
        macro_rules! merge_count_maps {
            ($($field:ident),+ $(,)?) => {
                $(merge_brep_counts(&mut self.asm.stats.$field, std::mem::take(&mut asm.stats.$field));)+
            };
        }
        merge_count_maps!(
            missing_face_surface_kinds,
            unknown_surface_kinds,
            procedural_curve_kinds,
            undecoded_pcurve_kinds,
            other_record_kinds,
        );
        let mut other = Self { asm, ..other };
        macro_rules! append_derived {
            ($($field:ident),+ $(,)?) => {
                $(self.$field.append(&mut other.$field);)+
            };
        }
        append_derived!(
            sketch_curve_links,
            persistent_design_links,
            persistent_subentity_tags,
            creation_timestamps,
        );
    }
}

/// Decode a framed active slice into the IR B-rep graph.
///
/// `stream` names the source ZIP entry for provenance. Ids are minted as
/// `<format>:brep:entity#<record-index>`, unique across the `RecordTable`.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    stream: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(decode_with_purpose(
        ctx,
        records,
        bytes,
        stream,
        format,
        DecodePurpose::Model,
    )?)
}

/// Decode a parsed text stream ([`cadmpeg_asm::sat`]) into the IR B-rep graph.
///
/// The text parser already typed the records and converted lengths into the
/// binary centimetre convention, so the shared decode path runs unchanged.
/// The header comes from the stream's ASCII header lines rather than a binary
/// header parse of `bytes`.
pub(crate) fn decode_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stream: &cadmpeg_asm::sat::TextStream,
    bytes: &[u8],
    entry: &str,
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(decode_with_header(
        ctx,
        &stream.records,
        bytes,
        Some(&stream.header.as_kernel_header()),
        entry,
        format,
        DecodePurpose::Model,
    )?)
}

/// Decode only the topology and analytic measurements used to bind ASM
/// history. Free-form carrier shapes are not materialized because historical
/// binding consumes their stable record identities, not their control data.
pub(crate) fn decode_history_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    format: IdFormat,
) -> Result<Brep, cadmpeg_core::CodecError> {
    Brep::from_asm(decode_with_purpose(
        ctx,
        records,
        bytes,
        "history",
        format,
        DecodePurpose::History,
    )?)
}

/// Decode a History subset against the complete state's subtype table.
pub(crate) fn decode_history_with_subtypes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    bytes: &[u8],
    subtypes: &cadmpeg_asm::nurbs::toks::SubtypeTable,
) -> Result<Brep, CodecError> {
    Brep::from_asm(cadmpeg_asm::brep::decode_history_with_subtypes(
        ctx,
        records,
        bytes,
        crate::ids::ID_FORMAT,
        subtypes,
    )?)
}

/// Resolve one Design body selector within one BREP blob. Exact native keys
/// take precedence; an absent key falls back to the zero-based body ordinal.
pub(crate) fn resolve_body_selector<'a>(
    body_keys: impl Iterator<Item = &'a BodyNativeKey> + Clone,
    selector: u64,
) -> Result<Option<&'a BodyId>, cadmpeg_core::CodecError> {
    let mut direct = None;
    for body in body_keys.clone() {
        if body.asm_body_key == Some(selector) {
            if direct.is_some() {
                return Err(CodecError::malformed(format_args!(
                    "F3D body selector {selector} matches multiple native body keys"
                )));
            }
            direct = Some(&body.body);
        }
    }
    if direct.is_some() {
        return Ok(direct);
    }
    let Ok(ordinal) = u32::try_from(selector) else {
        return Ok(None);
    };
    let mut matched = None;
    for body in body_keys {
        if body.body_ordinal == ordinal {
            if matched.is_some() {
                return Err(CodecError::malformed(format_args!(
                    "F3D body selector {selector} matches multiple body ordinals"
                )));
            }
            matched = Some(&body.body);
        }
    }
    Ok(matched)
}

/// The five members every `sketch_attrib_def` payload form writes.
struct SketchLinkPayload {
    sketch_curve_id: i64,
    ref_b: u64,
    sense: i64,
    role: i64,
    closure: i64,
}

/// Read the payload following a `sketch_attrib_def` family name.
///
/// The three header integers are `1`, `1`, and a form selector. Form `3` writes
/// the members as one tagged ASCII field with a `0` between the sense and the
/// role, form `2` as six integers with a trailing `0`, and form `0` as the five
/// members alone. All three write the same five members in the same order, so
/// each yields one link.
fn sketch_link_payload(values: &[AttributeValue]) -> Option<SketchLinkPayload> {
    let [AttributeValue::Integer(1), AttributeValue::Integer(1), AttributeValue::Integer(form), payload @ ..] =
        values
    else {
        return None;
    };
    match (*form, payload) {
        (3, [AttributeValue::String(field)]) => {
            let mut fields = field.split_ascii_whitespace();
            let (
                Some(sketch_curve_id),
                Some(ref_b),
                Some(sense),
                Some("0"),
                Some(role),
                Some(closure),
                None,
            ) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            )
            else {
                return None;
            };
            // `ref_b` reaches the full unsigned 64-bit range, so it is read
            // unsigned; every other member is signed.
            Some(SketchLinkPayload {
                sketch_curve_id: sketch_curve_id.parse().ok()?,
                ref_b: ref_b.parse().ok()?,
                sense: sense.parse().ok()?,
                role: role.parse().ok()?,
                closure: closure.parse().ok()?,
            })
        }
        (
            2,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure), AttributeValue::Integer(0)],
        )
        | (
            0,
            [AttributeValue::Integer(sketch_curve_id), AttributeValue::Integer(ref_b), AttributeValue::Integer(sense), AttributeValue::Integer(role), AttributeValue::Integer(closure)],
        ) => Some(SketchLinkPayload {
            sketch_curve_id: *sketch_curve_id,
            ref_b: u64::try_from(*ref_b).ok()?,
            sense: *sense,
            role: *role,
            closure: *closure,
        }),
        _ => None,
    }
}

/// Locate a Fusion family marker under the caller's scan and comparison allowance.
fn attribute_family(attribute: &SourceAttribute, family: &str) -> Option<usize> {
    for (index, value) in attribute.values.iter().enumerate() {
        if let AttributeValue::String(name) = value {
            if name == family {
                return Some(index);
            }
        }
    }
    None
}

fn sketch_curve_link(attribute: &SourceAttribute) -> Option<SketchCurveLink> {
    let family = attribute_family(attribute, "sketch_attrib_def")?;
    let payload = sketch_link_payload(&attribute.values[family + 1..])?;
    Some(SketchCurveLink {
        id: format!("f3d:design:sketch-curve-link#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        sketch_curve_id: payload.sketch_curve_id,
        ref_b: payload.ref_b,
        sense: crate::records::sketch_links::SketchLinkSense::try_from(payload.sense).ok(),
        role: payload.role,
        closure: payload.closure,
    })
}

fn persistent_design_links(
    attribute: &SourceAttribute,
) -> Result<Vec<PersistentDesignLink>, CodecError> {
    let AttributeTarget::Body(_) = &attribute.target else {
        return Ok(Vec::new());
    };
    let Some(GenericTagPayload {
        version,
        group_count,
        rest,
    }) = generic_tag_payload(attribute)
    else {
        return Ok(Vec::new());
    };
    let group_width = match version {
        GenericTagVersion::V2 => 4,
        GenericTagVersion::V3 => 5,
    };
    if group_count.checked_mul(group_width) != Some(rest.len()) {
        return Ok(Vec::new());
    }
    let mut links = Vec::new();
    for values in rest.chunks_exact(group_width) {
        let (entity_kind, design_id, design_reference) = match values {
            [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0)]
            | [AttributeValue::Integer(entity_kind), AttributeValue::String(design_id), AttributeValue::Integer(design_reference), AttributeValue::Integer(0), AttributeValue::Integer(0)] => {
                (*entity_kind, design_id, *design_reference)
            }
            _ => return Ok(Vec::new()),
        };
        let design_id_text = design_id.to_owned();
        let Ok(design_id) =
            crate::records::sketch_links::DesignPersistentIdText::try_from(design_id_text)
        else {
            return Ok(Vec::new());
        };
        if entity_kind != 3 {
            continue;
        }
        let ordinal = u32::try_from(links.len()).map_err(|_| {
            CodecError::Malformed("F3D persistent design link ordinal overflows".into())
        })?;
        let link = PersistentDesignLink {
            id: format!(
                "f3d:design:persistent-design-link#{}:{ordinal}",
                attribute_key(attribute)
            ),
            target: attribute.target.clone(),
            design_id,
            design_reference,
            ordinal,
        };
        links.push(link);
    }
    Ok(links)
}

fn persistent_subentity_tags(
    attribute: &SourceAttribute,
) -> Result<Vec<PersistentSubentityTag>, CodecError> {
    if !matches!(
        attribute.target,
        AttributeTarget::Face(_) | AttributeTarget::Edge(_)
    ) {
        return Ok(Vec::new());
    }
    let Some(GenericTagPayload {
        version,
        group_count,
        rest,
    }) = generic_tag_payload(attribute)
    else {
        return Ok(Vec::new());
    };
    // Each group consumes at least four leading attribute values from `rest`.
    let Ok(group_count) = u64::try_from(group_count) else {
        return Ok(Vec::new());
    };
    let Some(group_count) = bounded_len(group_count, 4, rest.len()) else {
        return Ok(Vec::new());
    };
    let mut position: usize = 0;
    let mut groups = Vec::new();
    for ordinal in 0..group_count {
        let Some(
            [AttributeValue::Integer(selector), AttributeValue::String(token), AttributeValue::Integer(0), AttributeValue::Integer(reference_count)],
        ) = rest.get(position..position + 4)
        else {
            return Ok(Vec::new());
        };
        let Some(token) = cadmpeg_core::text::NonBlankString::new(token.to_owned()) else {
            return Ok(Vec::new());
        };
        if *reference_count < 0 {
            return Ok(Vec::new());
        }
        let Ok(reference_count) = usize::try_from(*reference_count) else {
            return Ok(Vec::new());
        };
        let reference_start = position + 4;
        let Some(reference_end) = reference_start.checked_add(reference_count) else {
            return Ok(Vec::new());
        };
        let Some(reference_values) = rest.get(reference_start..reference_end) else {
            return Ok(Vec::new());
        };
        let mut design_references = Vec::new();
        for value in reference_values {
            let AttributeValue::Integer(value) = value else {
                return Ok(Vec::new());
            };
            design_references.push(*value);
        }
        if matches!(version, GenericTagVersion::V3) {
            if !matches!(rest.get(reference_end), Some(AttributeValue::Integer(0))) {
                return Ok(Vec::new());
            }
            position = reference_end + 1;
        } else {
            position = reference_end;
        }
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            CodecError::Malformed("F3D persistent subentity ordinal overflows".into())
        })?;
        let group = PersistentSubentityTag {
            id: format!(
                "f3d:design:persistent-subentity-tag#{}:{ordinal}",
                attribute_key(attribute)
            ),
            target: attribute.target.clone(),
            selector: *selector,
            token,
            design_references,
            ordinal,
        };
        groups.push(group);
    }
    if position != rest.len() {
        return Ok(Vec::new());
    }
    Ok(groups)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GenericTagVersion {
    V2,
    V3,
}

/// Generic-tag fields after the equal supported envelope versions are checked.
struct GenericTagPayload<'a> {
    version: GenericTagVersion,
    group_count: usize,
    rest: &'a [AttributeValue],
}

/// Return the common generic-tag version, group count, and payload.
///
/// The two leading integers are an equal envelope version. Versions two and
/// three select distinct, bounded group envelopes; a mixed or unsupported
/// pair is not a generic-tag envelope.
fn generic_tag_payload(attribute: &SourceAttribute) -> Option<GenericTagPayload<'_>> {
    let family = attribute_family(attribute, "generic_tag_attrib_def")?;
    let values = attribute.values.get(family + 1..)?;
    let [AttributeValue::Integer(left_version), AttributeValue::Integer(right_version), AttributeValue::Integer(-1), AttributeValue::String(marker), AttributeValue::Integer(group_count), rest @ ..] =
        values
    else {
        return None;
    };

    if left_version != right_version || marker != "generic_tag_attrib_def " || *group_count < 0 {
        return None;
    }
    let version = match *left_version {
        2 => GenericTagVersion::V2,
        3 => GenericTagVersion::V3,
        _ => return None,
    };
    let Ok(group_count) = usize::try_from(*group_count) else {
        return None;
    };
    Some(GenericTagPayload {
        version,
        group_count,
        rest,
    })
}

fn creation_timestamp(attribute: &SourceAttribute) -> Option<CreationTimestamp> {
    let family = attribute_family(attribute, "Timestamp_attrib_def")?;
    let marker = attribute.values.get(family + 1)?;
    if !matches!(marker, AttributeValue::Integer(1)) {
        return None;
    }
    let Some(AttributeValue::Float(unix_microseconds)) = attribute.values.get(family + 2) else {
        return None;
    };
    let record_index = attribute_key(attribute).parse().ok()?;
    Some(CreationTimestamp {
        id: format!("f3d:design:creation-timestamp#{}", attribute_key(attribute)),
        target: attribute.target.clone(),
        record_index,
        unix_microseconds: *unix_microseconds,
    })
}

#[cfg(test)]
mod tests;
