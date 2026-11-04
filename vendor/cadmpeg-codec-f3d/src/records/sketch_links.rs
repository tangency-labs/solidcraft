// SPDX-License-Identifier: Apache-2.0
//! Links from a design record to a sketch curve, and the persistent design-link table.

use cadmpeg_ir::attributes::AttributeTarget;
use serde::{Deserialize, Deserializer, Serialize};

cadmpeg_core::named_optional_field!(deserialize_sense, SketchLinkSense, "sense");
/// The `sketch_attrib_def` sense value that constrains nothing, written as the
/// unsigned decimal spelling of `0xFFFFFFFF`.
pub(crate) const SKETCH_LINK_SENSE_UNCONSTRAINED: i64 = 0xFFFF_FFFF;

/// Whether a `sketch_attrib_def` sense leaves the sense unconstrained. The
/// tagged-field form spells the value as the unsigned decimal of `0xFFFFFFFF`
/// and the integer forms as the signed `-1` of that same 32-bit pattern, so a
/// reader that accepts one spelling keeps the other as a stored sense.
fn sketch_link_sense_is_unconstrained(sense: i64) -> bool {
    sense == SKETCH_LINK_SENSE_UNCONSTRAINED || sense == -1
}

/// A stored sketch-link sense excluding the unconstrained sentinels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub(crate) struct SketchLinkSense(i64);

impl TryFrom<i64> for SketchLinkSense {
    type Error = &'static str;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if sketch_link_sense_is_unconstrained(value) {
            return Err("sense must omit the unconstrained sentinel");
        }
        Ok(Self(value))
    }
}

impl From<SketchLinkSense> for i64 {
    fn from(value: SketchLinkSense) -> Self {
        value.0
    }
}

/// Provenance link from a solved B-rep entity to its source sketch curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SketchCurveLink {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep entity this link provenances back to a sketch curve.
    pub(crate) target: AttributeTarget,
    /// First member of the component-scoped source-curve key.
    pub(crate) sketch_curve_id: i64,
    /// Secondary curve identity or owning sketch-text record index.
    /// The unsigned all-ones sentinel remains a distinct source qualifier.
    pub(crate) ref_b: u64,
    /// Which of the sketch curve's two senses this link takes, `0` or `1`.
    /// Absent when the source record leaves the sense unconstrained.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_sense"
    )]
    pub(crate) sense: Option<SketchLinkSense>,
    /// Source role tag distinguishing how the sketch curve participates in the link
    /// (e.g. profile edge vs. construction reference).
    pub(crate) role: i64,
    /// Source closure/continuity tag of the sketch curve at this link.
    pub(crate) closure: i64,
}

/// Nonempty decimal text identifying a persistent Design entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignPersistentIdText(String);

impl DesignPersistentIdText {
    /// Original decimal spelling of the identifier.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DesignPersistentIdText {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("design_id must contain one or more ASCII digits".into());
        }
        Ok(Self(value))
    }
}

impl From<DesignPersistentIdText> for String {
    fn from(value: DesignPersistentIdText) -> Self {
        value.0
    }
}

/// Persistent Fusion design identifier attached to a solved B-rep entity.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "PersistentDesignLinkWire")]
pub(crate) struct PersistentDesignLink {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep entity this persistent Fusion design id is attached to.
    pub(crate) target: AttributeTarget,
    /// Fusion persistent design-entity id string, stable across regeneration.
    pub(crate) design_id: DesignPersistentIdText,
    /// Design-stream reference paired with this persistent identifier.
    pub(crate) design_reference: i64,
    /// Position of this id in the entity's persistent-id history, in assignment order.
    pub(crate) ordinal: u32,
}

#[cfg(test)]
thread_local! {
    static PERSISTENT_DESIGN_LINK_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for PersistentDesignLink {
    fn clone(&self) -> Self {
        PERSISTENT_DESIGN_LINK_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            target: self.target.clone(),
            design_id: self.design_id.clone(),
            design_reference: self.design_reference,
            ordinal: self.ordinal,
        }
    }
}

/// The active persistent design link of every target: the highest-ordinal link
/// of that target's ordered run. Every other link of the run is a superseded
/// historical id retained for provenance.
#[cfg(test)]
pub(crate) fn current_persistent_design_links(
    links: &[PersistentDesignLink],
) -> std::collections::BTreeMap<&AttributeTarget, &PersistentDesignLink> {
    let mut current: std::collections::BTreeMap<&AttributeTarget, &PersistentDesignLink> =
        std::collections::BTreeMap::new();
    for link in links {
        current
            .entry(&link.target)
            .and_modify(|existing| {
                if link.ordinal > existing.ordinal {
                    *existing = link;
                }
            })
            .or_insert(link);
    }
    current
}

#[derive(Serialize, Deserialize)]
struct PersistentDesignLinkWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Solved B-rep entity this persistent Fusion design id is attached to.
    target: AttributeTarget,
    /// Fusion persistent design-entity id string, stable across regeneration.
    design_id: String,
    entity_kind: i64,
    /// Design-stream reference paired with this persistent identifier.
    design_reference: i64,
    /// Position of this id in the entity's persistent-id history, in assignment order.
    ordinal: u32,
}

impl Serialize for PersistentDesignLink {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            target: &'a AttributeTarget,
            design_id: &'a str,
            entity_kind: i64,
            design_reference: i64,
            ordinal: u32,
        }
        WireRef {
            id: &self.id,
            target: &self.target,
            design_id: self.design_id.as_str(),
            entity_kind: 3,
            design_reference: self.design_reference,
            ordinal: self.ordinal,
        }
        .serialize(serializer)
    }
}

impl TryFrom<PersistentDesignLinkWire> for PersistentDesignLink {
    type Error = String;

    fn try_from(wire: PersistentDesignLinkWire) -> Result<Self, Self::Error> {
        if wire.entity_kind != 3 {
            return Err("entity_kind must be 3".into());
        }
        Ok(Self {
            id: wire.id,
            target: wire.target,
            design_id: wire.design_id.try_into()?,
            design_reference: wire.design_reference,
            ordinal: wire.ordinal,
        })
    }
}

#[cfg(test)]
impl From<PersistentDesignLink> for PersistentDesignLinkWire {
    fn from(record: PersistentDesignLink) -> Self {
        Self {
            id: record.id,
            target: record.target,
            design_id: record.design_id.into(),
            entity_kind: 3,
            design_reference: record.design_reference,
            ordinal: record.ordinal,
        }
    }
}

/// Native face/edge tag group linking a solved subentity to design records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistentSubentityTag {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Solved B-rep face or edge carrying this tag group.
    pub(crate) target: AttributeTarget,
    /// Native selector stored before the tag token.
    pub(crate) selector: i64,
    /// Native UTF-8 tag token. Numeric strings and `-1` retain their spelling.
    #[serde(deserialize_with = "deserialize_persistent_tag_token")]
    pub(crate) token: cadmpeg_core::text::NonBlankString,
    /// Ordered signed Design-stream references carried by this group.
    pub(crate) design_references: Vec<i64>,
    /// Position of this group in the owning attribute record.
    pub(crate) ordinal: u32,
}

fn deserialize_persistent_tag_token<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<cadmpeg_core::text::NonBlankString, D::Error> {
    cadmpeg_core::text::NonBlankString::new(String::deserialize(deserializer)?)
        .ok_or_else(|| serde::de::Error::custom("token must not be empty"))
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
