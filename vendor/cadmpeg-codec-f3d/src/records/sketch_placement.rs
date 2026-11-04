// SPDX-License-Identifier: Apache-2.0
//! Sketch visibility, placement and the frame a sketch is placed on.

use super::identity::{DesignAffineTransform, DesignEntityId, IDENTITY_MATRIX};
use super::references::DesignClassTag;
use crate::layout::sketch_typed_compact_member_head as typed_compact;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

cadmpeg_core::named_optional_field!(deserialize_scope_record_index, u32, "scope_record_index");
cadmpeg_core::named_optional_field!(deserialize_transform_offset, u64, "transform_offset");
cadmpeg_core::named_optional_field!(deserialize_visibility, DesignSketchVisibility, "visibility");
cadmpeg_core::named_optional_field!(
    deserialize_ordinal_collection,
    std::num::NonZeroU64,
    "ordinal_collection"
);
/// Typed sketch-container visibility bound to a Design sketch entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignSketchVisibilityWire",
    into = "DesignSketchVisibilityWire"
)]
pub(crate) struct DesignSketchVisibility {
    /// One-based ordinal among sketch Geometry members in the referenced collection.
    pub(crate) stream_ordinal: NonZeroU32,
    stream_ordinal_offset: u64,
    /// Direct display visibility.
    pub(crate) visible: bool,
    pub(crate) ordinal_collection: Option<std::num::NonZeroU64>,
}

impl DesignSketchVisibility {
    pub(crate) fn new(
        stream_ordinal: NonZeroU32,
        stream_ordinal_offset: u64,
        visible: bool,
    ) -> Result<Self, String> {
        stream_ordinal_offset
            .checked_add(5)
            .ok_or("stream_ordinal_offset overflows visible_offset")?;
        Ok(Self {
            stream_ordinal,
            stream_ordinal_offset,
            visible,
            ordinal_collection: None,
        })
    }

    pub(crate) fn stream_ordinal_offset(&self) -> u64 {
        self.stream_ordinal_offset
    }

    pub(crate) fn visible_offset(&self) -> u64 {
        self.stream_ordinal_offset + 5
    }
}

#[derive(Serialize, Deserialize)]
struct DesignSketchVisibilityWire {
    stream_ordinal: u32,
    stream_ordinal_offset: u64,
    visible_offset: u64,
    visible: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ordinal_collection"
    )]
    ordinal_collection: Option<std::num::NonZeroU64>,
}

impl TryFrom<DesignSketchVisibilityWire> for DesignSketchVisibility {
    type Error = String;

    fn try_from(wire: DesignSketchVisibilityWire) -> Result<Self, Self::Error> {
        let mut value = Self::new(
            NonZeroU32::new(wire.stream_ordinal).ok_or("stream_ordinal must be nonzero")?,
            wire.stream_ordinal_offset,
            wire.visible,
        )?;
        value.ordinal_collection = wire.ordinal_collection;
        if wire.visible_offset != value.visible_offset() {
            return Err("visible_offset must equal stream_ordinal_offset + 5".into());
        }
        Ok(value)
    }
}

impl From<DesignSketchVisibility> for DesignSketchVisibilityWire {
    fn from(value: DesignSketchVisibility) -> Self {
        Self {
            stream_ordinal: value.stream_ordinal.get(),
            stream_ordinal_offset: value.stream_ordinal_offset(),
            visible_offset: value.visible_offset(),
            visible: value.visible,
            ordinal_collection: value.ordinal_collection,
        }
    }
}

/// Local-to-model placement frame referenced by a Design sketch scope.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignSketchPlacementWire")]
pub(crate) struct DesignSketchPlacement {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Owning parameter scope, when the sketch has a parameter scope.
    pub(crate) scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    pub(crate) entity_id: DesignEntityId,
    /// Typed sketch-container visibility for the placed sketch entity.
    pub(crate) visibility: Option<DesignSketchVisibility>,
    /// Source dynamic three-digit ASCII primary class tag.
    pub(crate) class_tag: DesignClassTag,
    /// Shared logical record identity.
    pub(crate) record_index: u32,
    /// Source dynamic class tag of the paired header.
    pub(crate) paired_class_tag: DesignClassTag,
    /// Source layout and its checked placement matrix and byte extent.
    pub(crate) frame: DesignSketchFrame,
}

#[cfg(test)]
thread_local! {
    static SKETCH_PLACEMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignSketchPlacement {
    fn clone(&self) -> Self {
        SKETCH_PLACEMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            scope_record_index: self.scope_record_index,
            entity_id: self.entity_id.clone(),
            visibility: self.visibility,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            paired_class_tag: self.paired_class_tag.clone(),
            frame: self.frame.clone(),
        }
    }
}

pub(crate) fn valid_sketch_transform(transform: &[[f64; 4]; 4]) -> bool {
    const EPSILON: f64 = 1.0e-10;
    if !transform.iter().flatten().all(|value| value.is_finite())
        || transform[3] != [0.0, 0.0, 0.0, 1.0]
    {
        return false;
    }
    let columns = [
        [transform[0][0], transform[1][0], transform[2][0]],
        [transform[0][1], transform[1][1], transform[2][1]],
        [transform[0][2], transform[1][2], transform[2][2]],
    ];
    for (ordinal, column) in columns.iter().enumerate() {
        let norm = column.iter().map(|value| value * value).sum::<f64>();
        if (norm - 1.0).abs() > EPSILON {
            return false;
        }
        for other in &columns[..ordinal] {
            let dot = column
                .iter()
                .zip(other)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > EPSILON {
                return false;
            }
        }
    }
    true
}

/// A finite affine placement with orthonormal basis columns.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct SketchPlacementMatrix(DesignAffineTransform);

impl SketchPlacementMatrix {
    /// The identity placement.
    pub(crate) const IDENTITY: Self = Self(DesignAffineTransform::IDENTITY);
    /// The row-major matrix coefficients.
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
    /// The matrix rows in storage order.
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, [f64; 4]> {
        self.0.iter()
    }
}

impl AsRef<[[f64; 4]; 4]> for SketchPlacementMatrix {
    fn as_ref(&self) -> &[[f64; 4]; 4] {
        &self.0
    }
}

impl std::ops::Index<usize> for SketchPlacementMatrix {
    type Output = [f64; 4];
    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl From<SketchPlacementMatrix> for [[f64; 4]; 4] {
    fn from(matrix: SketchPlacementMatrix) -> Self {
        matrix.0.rows()
    }
}

impl TryFrom<[[f64; 4]; 4]> for SketchPlacementMatrix {
    type Error = String;
    fn try_from(value: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        if valid_sketch_transform(&value) {
            Ok(Self(DesignAffineTransform::try_from(value)?))
        } else {
            Err("transform must be a finite affine matrix with orthonormal columns".into())
        }
    }
}

/// The matrix-bearing payload of each source placement layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DesignSketchFrameForm {
    ScopeCompact,
    ScopeGenesisCompact,
    ScopeLegacy305(SketchPlacementMatrix),
    ScopeLegacy325(SketchPlacementMatrix),
    ScopeExplicit(SketchPlacementMatrix),
    ScopeGenesisExplicit(SketchPlacementMatrix),
    MemberCompact {
        paired_byte_offset: u64,
    },
    MemberTypedCompact {
        paired_byte_offset: u64,
    },
    MemberExplicit {
        paired_byte_offset: u64,
        transform: SketchPlacementMatrix,
        /// The closing head reference carries an inline type GUID, making
        /// the frame forty bytes longer.
        typed_reference: bool,
    },
}

/// A placement layout whose complete byte extent fits in its address space.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignSketchFrame {
    byte_offset: u64,
    form: DesignSketchFrameForm,
}

impl DesignSketchFrame {
    pub(crate) fn form(&self) -> &DesignSketchFrameForm {
        &self.form
    }
    pub(crate) fn new(byte_offset: u64, form: DesignSketchFrameForm) -> Result<Self, String> {
        let value = Self { byte_offset, form };
        byte_offset
            .checked_add(value.frame_length())
            .ok_or("byte_offset overflows the sketch frame extent")?;
        Ok(value)
    }
    fn frame_length(&self) -> u64 {
        match self.form {
            DesignSketchFrameForm::ScopeCompact => 201,
            DesignSketchFrameForm::ScopeGenesisCompact => 213,
            DesignSketchFrameForm::ScopeLegacy305(_) => 305,
            DesignSketchFrameForm::ScopeLegacy325(_) => 325,
            DesignSketchFrameForm::ScopeExplicit(_) => 329,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 341,
            DesignSketchFrameForm::MemberCompact { .. } => 34,
            DesignSketchFrameForm::MemberTypedCompact { .. } => {
                cadmpeg_core::decode::u64_from_index(typed_compact::LEN)
            }
            DesignSketchFrameForm::MemberExplicit {
                typed_reference, ..
            } => {
                if typed_reference {
                    202
                } else {
                    162
                }
            }
        }
    }
    fn transform(&self) -> &[[f64; 4]; 4] {
        match &self.form {
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. }
            | DesignSketchFrameForm::MemberTypedCompact { .. } => &IDENTITY_MATRIX,
            DesignSketchFrameForm::ScopeLegacy305(matrix)
            | DesignSketchFrameForm::ScopeLegacy325(matrix)
            | DesignSketchFrameForm::ScopeExplicit(matrix)
            | DesignSketchFrameForm::ScopeGenesisExplicit(matrix)
            | DesignSketchFrameForm::MemberExplicit {
                transform: matrix, ..
            } => &matrix.0,
        }
    }
    fn transform_offset(&self) -> Option<u64> {
        let relative = match self.form {
            DesignSketchFrameForm::ScopeCompact
            | DesignSketchFrameForm::ScopeGenesisCompact
            | DesignSketchFrameForm::MemberCompact { .. }
            | DesignSketchFrameForm::MemberTypedCompact { .. } => return None,
            DesignSketchFrameForm::ScopeLegacy305(_) | DesignSketchFrameForm::ScopeLegacy325(_) => {
                48
            }
            DesignSketchFrameForm::ScopeExplicit(_) => 55,
            DesignSketchFrameForm::ScopeGenesisExplicit(_) => 66,
            DesignSketchFrameForm::MemberExplicit { .. } => 22,
        };
        Some(self.byte_offset + relative)
    }
    pub(super) fn paired_byte_offset(&self) -> u64 {
        match self.form {
            DesignSketchFrameForm::MemberCompact { paired_byte_offset }
            | DesignSketchFrameForm::MemberTypedCompact { paired_byte_offset }
            | DesignSketchFrameForm::MemberExplicit {
                paired_byte_offset, ..
            } => paired_byte_offset,
            _ => self.byte_offset + self.frame_length(),
        }
    }
}

impl DesignSketchPlacement {
    pub(crate) fn byte_offset(&self) -> u64 {
        self.frame.byte_offset
    }
    pub(crate) fn frame_length(&self) -> u64 {
        self.frame.frame_length()
    }
    pub(crate) fn transform(&self) -> &[[f64; 4]; 4] {
        self.frame.transform()
    }
    pub(crate) fn transform_offset(&self) -> Option<u64> {
        self.frame.transform_offset()
    }
    pub(crate) fn paired_byte_offset(&self) -> u64 {
        self.frame.paired_byte_offset()
    }
    pub(crate) fn member_run_head(&self) -> bool {
        matches!(
            self.frame.form,
            DesignSketchFrameForm::MemberCompact { .. }
                | DesignSketchFrameForm::MemberTypedCompact { .. }
                | DesignSketchFrameForm::MemberExplicit { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignSketchPlacementWire {
    /// Globally unique deterministic identifier for this native record.
    id: String,
    /// Owning parameter-scope record; absent when the sketch has no parameter
    /// scope. A localized Sketch scope can own a member-run head placement
    /// through record interval order without directly referencing it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_scope_record_index"
    )]
    scope_record_index: Option<u32>,
    /// Full Design entity id of the placed sketch.
    entity_id: String,
    /// Typed sketch-container visibility for the placed sketch entity.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visibility"
    )]
    visibility: Option<DesignSketchVisibility>,
    /// Byte offset of the primary indexed record header.
    byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII primary class tag.
    class_tag: String,
    /// Shared logical record identity.
    record_index: u32,
    /// Byte length from the primary header to the paired header.
    frame_length: u64,
    /// Row-major local-to-model affine transform.
    transform: [[f64; 4]; 4],
    /// Byte offset of the explicit 16-f64 matrix; absent for the compact identity form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform_offset"
    )]
    transform_offset: Option<u64>,
    /// Per-file dynamic class tag of the paired header.
    paired_class_tag: String,
    /// Byte offset of the paired indexed record header.
    paired_byte_offset: u64,
    /// Whether this placement is the transform-carrying member-run head
    /// record named by the sketch entity's paired record rather than a
    /// parameter-scope placement frame.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    member_run_head: bool,
}

impl Serialize for DesignSketchPlacement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WireRef<'a> {
            id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            scope_record_index: Option<u32>,
            entity_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            visibility: Option<DesignSketchVisibility>,
            byte_offset: u64,
            class_tag: &'a str,
            record_index: u32,
            frame_length: u64,
            transform: [[f64; 4]; 4],
            #[serde(skip_serializing_if = "Option::is_none")]
            transform_offset: Option<u64>,
            paired_class_tag: &'a str,
            paired_byte_offset: u64,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            member_run_head: bool,
        }
        WireRef {
            id: &self.id,
            scope_record_index: self.scope_record_index,
            entity_id: self.entity_id.as_str(),
            visibility: self.visibility,
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            frame_length: self.frame_length(),
            transform: *self.transform(),
            transform_offset: self.transform_offset(),
            paired_class_tag: self.paired_class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset(),
            member_run_head: self.member_run_head(),
        }
        .serialize(serializer)
    }
}

impl TryFrom<DesignSketchPlacementWire> for DesignSketchPlacement {
    type Error = String;
    fn try_from(wire: DesignSketchPlacementWire) -> Result<Self, Self::Error> {
        use DesignSketchFrameForm as Form;
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;

        let form = match (wire.member_run_head, wire.frame_length) {
            (false, 201) => Form::ScopeCompact,
            (false, 213) => Form::ScopeGenesisCompact,
            (false, 305) => Form::ScopeLegacy305(wire.transform.try_into()?),
            (false, 325) => Form::ScopeLegacy325(wire.transform.try_into()?),
            (false, 329) => Form::ScopeExplicit(wire.transform.try_into()?),
            (false, 341) => Form::ScopeGenesisExplicit(wire.transform.try_into()?),
            (true, 34) => Form::MemberCompact {
                paired_byte_offset: wire.paired_byte_offset,
            },
            (true, length)
                if length == cadmpeg_core::decode::u64_from_index(typed_compact::LEN) =>
            {
                Form::MemberTypedCompact {
                    paired_byte_offset: wire.paired_byte_offset,
                }
            }
            (true, length @ (162 | 202)) => Form::MemberExplicit {
                paired_byte_offset: wire.paired_byte_offset,
                transform: wire.transform.try_into()?,
                typed_reference: length == 202,
            },
            _ => return Err("frame_length is invalid for member_run_head".into()),
        };
        let frame = DesignSketchFrame::new(wire.byte_offset, form)?;
        if frame
            .transform()
            .iter()
            .flatten()
            .zip(wire.transform.iter().flatten())
            .any(|(left, right)| left.to_bits() != right.to_bits())
        {
            return Err("transform disagrees with the compact frame identity".into());
        }
        if wire.transform_offset != frame.transform_offset() {
            return Err("transform_offset disagrees with the sketch frame layout".into());
        }
        if wire.paired_byte_offset != frame.paired_byte_offset() {
            return Err("paired_byte_offset disagrees with the sketch frame extent".into());
        }
        Ok(Self {
            id: wire.id,
            scope_record_index: wire.scope_record_index,
            entity_id,
            visibility: wire.visibility,
            class_tag: DesignClassTag::try_from(wire.class_tag)?,
            record_index: wire.record_index,
            paired_class_tag: DesignClassTag::try_from(wire.paired_class_tag)
                .map_err(|error| format!("paired_class_tag: {error}"))?,
            frame,
        })
    }
}

#[cfg(test)]
impl From<DesignSketchPlacement> for DesignSketchPlacementWire {
    fn from(value: DesignSketchPlacement) -> Self {
        let byte_offset = value.byte_offset();
        let frame_length = value.frame_length();
        let transform = *value.transform();
        let transform_offset = value.transform_offset();
        let paired_byte_offset = value.paired_byte_offset();
        let member_run_head = value.member_run_head();
        Self {
            id: value.id,
            scope_record_index: value.scope_record_index,
            entity_id: value.entity_id.into_string(),
            visibility: value.visibility,
            byte_offset,
            class_tag: value.class_tag.into(),
            record_index: value.record_index,
            frame_length,
            transform,
            transform_offset,
            paired_class_tag: value.paired_class_tag.into(),
            paired_byte_offset,
            member_run_head,
        }
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
